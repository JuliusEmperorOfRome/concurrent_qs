pub mod spsc;

#[cfg(test)]
mod tests {
    use super::spsc::Queue;
    use std::thread;

    #[test]
    fn mt_ref_test() {
        static mut Q: Queue<i32, 4> = Queue::new();
        let (src, sink) = unsafe { Q.ref_split() };

        let thread = thread::spawn(move || {
            for i in 0..256 {
                loop {
                    if let Some(item) = sink.pop() {
                        assert_eq!(item, i);
                        break;
                    }
                    std::thread::yield_now();
                }
            }
        });

        for i in 0..256 {
            while let Err(_) = src.push(i) {}
        }

        thread.join().expect("Failed to join thread.");
    }

    #[test]
    fn mt_arc_test() {
        let (src, sink) = Queue::<i32, 4>::new().split();

        let thread = thread::spawn(move || {
            for i in 0..256 {
                loop {
                    if let Some(item) = sink.pop() {
                        assert_eq!(item, i);
                        break;
                    }
                    std::thread::yield_now();
                }
            }
        });

        for i in 0..256 {
            while let Err(_) = src.push(i) {}
        }

        thread.join().expect("Failed to join thread.");
    }

    #[test]
    fn st_ref_test() {
        let mut q = Queue::<i32, 4>::new();
        let (src, sink) = q.ref_split();

        assert_eq!(sink.pop(), None);

        assert_eq!(src.push(1), Ok(()));
        assert_eq!(src.push(2), Ok(()));
        assert_eq!(src.push(3), Ok(()));
        assert_eq!(src.push(4), Ok(()));
        assert_eq!(src.push(5), Err(5));

        assert_eq!(sink.pop(), Some(1));
        assert_eq!(sink.pop(), Some(2));
        assert_eq!(sink.pop(), Some(3));
        assert_eq!(sink.pop(), Some(4));
        assert_eq!(sink.pop(), None);
    }

    #[test]
    fn st_arc_test() {
        let (src, sink) = Queue::<i32, 4>::new().split();

        assert_eq!(sink.pop(), None);

        assert_eq!(src.push(1), Ok(()));
        assert_eq!(src.push(2), Ok(()));
        assert_eq!(src.push(3), Ok(()));
        assert_eq!(src.push(4), Ok(()));
        assert_eq!(src.push(5), Err(5));

        assert_eq!(sink.pop(), Some(1));
        assert_eq!(sink.pop(), Some(2));
        assert_eq!(sink.pop(), Some(3));
        assert_eq!(sink.pop(), Some(4));
        assert_eq!(sink.pop(), None);
    }
}
