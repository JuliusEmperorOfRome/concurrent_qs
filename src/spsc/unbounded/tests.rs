cfg_not_loom! {

#[test]
fn drop() {
    use std::rc::Rc;
    let rc = Rc::new(());
    {
        let (src, _sink) = super::channel();
        src.send(rc.clone()).unwrap();
        src.send(rc.clone()).unwrap();
        src.send(rc.clone()).unwrap();
        src.send(rc.clone()).unwrap();
        src.send(rc.clone()).unwrap();
    }
    assert_eq!(Rc::strong_count(&rc), 1);
}

#[test]
fn order() {
    let (src, sink) = super::channel::<u8>();
    std::thread::spawn(move || {
        for i in 0..10 {
            src.send(i).unwrap();
        }
    });
    for i in 0..10 {
        assert_eq!(sink.recv().unwrap(), i);
    }
}

#[test]
fn sender_dc() {
    let (src, sink) = super::channel::<()>();
    std::thread::spawn(move || {
        std::mem::drop(src);
    });
    assert_eq!(sink.recv(), Err(super::RecvError {}));
}

#[test]
fn receiver_dc() {
    let (src, sink) = super::channel::<()>();
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
    static DROPPED: AtomicBool = AtomicBool::new(false);
    std::thread::spawn(move || {
        std::mem::drop(sink);
        DROPPED.store(true, SeqCst);
    });

    while !DROPPED.load(SeqCst) {
        std::thread::yield_now();
    }
    src.send(()).unwrap_err();
}

}

cfg_loom! {

#[test]
fn drop() {
    loom::model(|| {
        use loom::sync::Arc;
        let arc = Arc::new(());
        {
            let arc = arc.clone();
            let (src, sink) = super::channel();
            let handle = loom::thread::spawn(move || {
                for _ in 0..3 {
                    src.send(arc.clone()).unwrap();
                }
            });
            if sink.recv().is_ok() {
                handle.join().unwrap();
            }
        }
        assert_eq!(Arc::strong_count(&arc), 1);
    });
}

#[test]
fn order() {
    let mut builder = loom::model::Builder::new();
    builder.max_threads = 2;
    builder.preemption_bound = Some(4);
    builder.check(|| {
        let (src, sink) = super::channel::<u8>();
        loom::thread::spawn(move || {
            for i in 0..3 {
                src.send(i).unwrap();
            }
        });
        for i in 0..3 {
            assert_eq!(sink.recv().unwrap_or_else(|_| panic!("failed at {i}")), i);
        }
    });
}

#[test]
fn sender_dc() {
    loom::model(|| {
        let (src, sink) = super::channel::<()>();
        loom::thread::spawn(move || {
            std::mem::drop(src);
        });
        assert_eq!(sink.recv(), Err(super::RecvError {}))
    });
}

#[test]
fn receiver_dc() {
    loom::model(|| {
        let (src, sink) = super::channel::<()>();
        use std::sync::Arc;
        let dropped = Arc::new(());
        {
            let dropped = dropped.clone();
            loom::thread::spawn(move || {
                let _dropped = dropped;
                std::mem::drop(sink);
            });
        }

        while Arc::strong_count(&dropped) != 1 {
            loom::thread::yield_now();
        }
        src.send(()).unwrap_err();
    });
}

}
