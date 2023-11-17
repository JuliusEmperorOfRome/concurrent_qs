use crate::alloc::{self, Layout};
use crate::cell::UnsafeCell;
use crate::error::{RecvError, SendError, TryRecvError};
use crate::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};
use crate::sync::atomic::{AtomicPtr, AtomicUsize};

use crate::util::ann::AtomicNonNull;
use crate::util::cache::CacheAligned;
use crate::util::park::Parker;

use std::cell::Cell;
use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

/*
 * unbounded::channel uses a singly-linked list structured like this:
 *
 * |<-----------`value` is uninit----------->|<--`value` is init-->|
 * `sender.next_for_reuse` -> ... ->  `tail` -> ... -> `sender.head`
 */
pub(super) struct Inner<T> {
    sender: CacheAligned<SenderData<T>>,
    tail: CacheAligned<AtomicNonNull<Node<T>>>,
    // Sender "drops" twice, to allow unpark with drop_count != 0.
    pub(super) drop_count: AtomicUsize,
}

struct SenderData<T> {
    head: Cell<NonNull<Node<T>>>,
    next_for_reuse: Cell<NonNull<Node<T>>>,
    tail_cache: Cell<NonNull<Node<T>>>,
    park_receiver: Parker, //Parkers are accessed by wakers more often than the parked thread
}

struct Node<T> {
    next: AtomicPtr<Node<T>>,
    value: UnsafeCell<MaybeUninit<T>>,
}

impl<T> Inner<T> {
    pub(super) fn peer_connected(&self) -> bool {
        self.drop_count.load(Acquire) == 0
    }

    pub(super) fn send(&self, item: T) -> Result<(), SendError<T>> {
        if self.drop_count.load(Relaxed) != 0 {
            Err(SendError(item))
        } else {
            //SAFETY: nodes live until Inner::drop
            let node = unsafe { self.next_node().as_ref() };

            node.value.with_mut(|val| unsafe {
                /*SAFETY:
                 * - nodes from `self.next_node()` always have uninit values
                 * - MaybeUninit<T> has the same layout as T
                 */
                (val as *mut T).write(item)
            });

            let old = self.sender.head.replace(node.into());
            // SAFETY: nodes live until Inner::drop
            unsafe { old.as_ref() }
                .next
                .store(node as *const _ as *mut _, Release);

            self.unpark_receiver();
            Ok(())
        }
    }

    pub(super) fn try_recv(&self) -> Result<T, TryRecvError> {
        //SAFETY: nodes live until Inner::drop
        let tail = unsafe { self.tail.load(Relaxed).as_ref() };

        let new_tail = match NonNull::new(tail.next.load(Acquire)) {
            Some(p) => p,
            None => match self.drop_count.load(Acquire) {
                0 => return Err(TryRecvError::Empty),
                _ => match NonNull::new(tail.next.load(Acquire)) {
                    Some(p) => p,
                    None => return Err(TryRecvError::Disconnected),
                },
            },
        };

        //SAFETY: nodes live until Inner::drop
        let new_tail = unsafe { new_tail.as_ref() };
        let ret = new_tail.value.with_mut(|x| unsafe {
            /*SAFETY: inserted nodes have initialised values*/
            x.read().assume_init()
        });

        self.tail.store(new_tail.into(), Release);

        Ok(ret)
    }

    pub(super) fn recv(&self) -> Result<T, RecvError> {
        loop {
            match self.try_recv() {
                Ok(t) => return Ok(t),
                Err(TryRecvError::Disconnected) => return Err(RecvError {}),
                Err(TryRecvError::Empty) => unsafe {
                    //SAFETY: only Receiver parks and it's !Copy + !Clone + !Sync
                    self.sender.park_receiver.park()
                },
            }
        }
    }

    pub(super) fn unpark_receiver(&self) {
        self.sender.park_receiver.unpark();
    }

    pub(super) fn allocate() -> (InnerHolder<T>, InnerHolder<T>) {
        let this = Self::new();
        //SAFETY: deallocated in InnerHolder::drop
        let store_self = unsafe { alloc::alloc(Layout::new::<Self>()) as *mut Self };
        let store_self = NonNull::new(store_self).expect("failed to allocate memory");
        //SAFETY: checked for null and uninit
        unsafe { store_self.as_ptr().write(this) };

        (InnerHolder(store_self), InnerHolder(store_self))
    }

    fn new() -> Self {
        let node = unsafe {
            //SAFETY: released in Drop
            Node::create()
        };
        Self {
            sender: CacheAligned::new(SenderData {
                head: Cell::new(node),
                next_for_reuse: Cell::new(node),
                tail_cache: Cell::new(node),
                park_receiver: Parker::new(),
            }),
            tail: CacheAligned::new(AtomicNonNull::new(node)),
            drop_count: AtomicUsize::new(0),
        }
    }

    fn next_node_fast(&self) -> Option<NonNull<Node<T>>> {
        if self.sender.next_for_reuse != self.sender.tail_cache {
            let node = self.sender.next_for_reuse.get();
            /*SAFETY:
             * this node has passed Receiver, and only the Sender: !Copy + !Clone + !Sync
             * has access to it, so this is the only reference.
             */
            let next = unsafe {
                Node::with_mut_next(node, |next| std::mem::replace(next, ptr::null_mut()))
            };
            //SAFETY: nodes before head have non null next
            let next = unsafe {
                debug_assert_ne!(next, ptr::null_mut());
                NonNull::new_unchecked(next)
            };

            self.sender.next_for_reuse.set(next);

            Some(node)
        } else {
            None
        }
    }

    fn next_node(&self) -> NonNull<Node<T>> {
        match self.next_node_fast() {
            Some(p) => p,
            None => {
                self.sender.tail_cache.set(self.tail.load(Acquire));
                match self.next_node_fast() {
                    Some(p) => p,
                    //SAFETY: deallocated in `drop`
                    None => unsafe { Node::create() },
                }
            }
        }
    }
}

impl<T> Drop for Inner<T> {
    fn drop(&mut self) {
        let mut current = self.sender.next_for_reuse.get();
        let tail = self.tail.with_mut(|x| *x);

        loop {
            /*SAFETY
             * - nodes never leave their Inner
             * - drop has exclusive access to this Inner
             */
            let next = unsafe { Node::with_mut_next(current, |x| *x) };
            /*SAFETY
             * - nodes are only created with `create`
             * - `release` only called in `drop`
             * - nodes between `next_for_reuse` and `tail` have uninit values
             */
            unsafe { Node::release(current) }

            if current == tail {
                match NonNull::new(next) {
                    Some(next) => break current = next,
                    None => return,
                }
            }

            //SAFETY: only head->next is null, and head comes after (or is the same as) tail
            current = unsafe {
                debug_assert!(!next.is_null());
                NonNull::new_unchecked(next)
            }
        }

        loop {
            // cfg tail doesn't work, fake loop
            let next = loop {
                //SAFETY: current is still alive
                let node = unsafe { current.as_mut() };

                node.value.with_mut(|x| unsafe {
                    //SAFETY: all values past tail have values
                    (x as *mut T).drop_in_place()
                });

                #[cfg(not(feature = "loom"))]
                break *node.next.get_mut();
                #[cfg(feature = "loom")]
                break node.next.with_mut(|x| *x);
            };

            /*SAFETY
             * - nodes are only created with `create`
             * - `release` only called in `drop`
             * - value already dropped
             */
            unsafe { Node::release(current) }

            match NonNull::new(next) {
                Some(next) => current = next,
                None => break,
            }
        }
    }
}

impl<T> Node<T> {
    const LAYOUT: Layout = Layout::new::<Self>();
    /// Creates a new heap allocated node.
    ///
    /// # Safety
    ///
    /// If the returned node isn't later passed to
    /// `release`, the memory leaks.
    unsafe fn create() -> NonNull<Self> {
        let res =
            NonNull::new(alloc::alloc(Self::LAYOUT) as *mut Node<T>).expect("allocation failed");
        //SAFETY: allocated with correct layout and checked for null
        ptr::write(
            res.as_ptr(),
            Node {
                next: AtomicPtr::new(ptr::null_mut()),
                value: UnsafeCell::new(MaybeUninit::uninit()),
            },
        );

        res
    }

    /// Releases the node.
    ///
    /// # Safety
    ///
    /// - Must have been `create`d and not `release`d before.
    /// - The caller is responsible for dropping `value`.
    #[inline(always)]
    unsafe fn release(node: NonNull<Self>) {
        alloc::dealloc(node.as_ptr() as *mut u8, Self::LAYOUT);
    }

    /// # Safety
    ///
    /// - `node`.as_mut must be valid (See [`core::ptr::NonNull`])
    #[inline(always)]
    unsafe fn with_mut_next<R>(node: NonNull<Self>, f: impl Fn(&mut *mut Self) -> R) -> R {
        #[cfg(not(feature = "loom"))]
        return f((&mut *node.as_ptr()).next.get_mut());
        #[cfg(feature = "loom")]
        return (&mut *node.as_ptr()).next.with_mut(f);
    }
}

pub(super) struct InnerHolder<T>(NonNull<Inner<T>>);

impl<T> core::ops::Deref for InnerHolder<T> {
    type Target = Inner<T>;
    fn deref(&self) -> &Self::Target {
        //SAFETY: Valid at least until InnerHolder::drop
        unsafe { self.0.as_ref() }
    }
}

impl<T> Drop for InnerHolder<T> {
    fn drop(&mut self) {
        match self.drop_count.fetch_add(1, AcqRel) {
            0 | 1 => { /*some references still exist*/ }
            2 => {
                //happens only once, since drop count never decrements

                let inner_ptr = self.0.as_ptr();
                //SAFETY: inner still lives, happens once
                unsafe { inner_ptr.drop_in_place() };
                //SAFETY: allocated in Inner::allocate, happens once
                unsafe { alloc::dealloc(inner_ptr as *mut _, Layout::new::<Inner<T>>()) };
            }
            _ => unreachable!(),
        }
    }
}
