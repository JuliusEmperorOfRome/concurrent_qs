use super::*;
cfg_not_loom! {
#[test]
fn st_insert_remove() {
    let (src, sink) = channel::<i32>(4);

    assert_eq!(src.try_send(1), Ok(()));
    assert_eq!(src.try_send(2), Ok(()));
    assert_eq!(src.try_send(3), Ok(()));
    assert_eq!(src.try_send(4), Ok(()));
    assert_eq!(src.try_send(5), Err(TrySendError::Full(5)));

    assert_eq!(sink.try_recv(), Ok(1));
    assert_eq!(sink.try_recv(), Ok(2));
    assert_eq!(sink.try_recv(), Ok(3));
    assert_eq!(sink.try_recv(), Ok(4));
    assert_eq!(sink.try_recv(), Err(TryRecvError::Empty));
}

#[test]
fn st_insert_remove_blocking() {
    let (src, sink) = channel::<i32>(4);

    assert_eq!(src.send(1), Ok(()));
    assert_eq!(src.send(2), Ok(()));
    assert_eq!(src.send(3), Ok(()));
    assert_eq!(src.send(4), Ok(()));

    assert_eq!(sink.recv(), Ok(1));
    assert_eq!(sink.recv(), Ok(2));
    assert_eq!(sink.recv(), Ok(3));
    assert_eq!(sink.recv(), Ok(4));
}

#[test]
fn st_sender_disconnect() {
    let (src, sink) = channel::<i32>(0);
    drop(src);
    assert_eq!(sink.try_recv(), Err(TryRecvError::Disconnected));
}
#[test]
fn st_receiver_disconnect() {
    let (src, sink) = channel::<i32>(0);
    drop(sink);
    assert_eq!(src.try_send(1), Err(TrySendError::Disconnected(1)));
}

#[test]
fn send_non_copy() {
    let (src, _sink) = channel::<Box<str>>(1);
    src.send("Hello".to_owned().into_boxed_str()).unwrap();
}

}

cfg_loom! {
use loom::model::model;
use loom::thread;

#[test]
fn mt_sender_disconnect() {
    model(|| {
        let (src, sink) = channel::<i32>(1); //minimize the time loom takes.
        thread::spawn(|| drop(src));
        loop {
            match sink.try_recv() {
                Ok(_) => panic!("No data was sent, but some was received."),
                Err(TryRecvError::Empty) => thread::yield_now(),
                Err(TryRecvError::Disconnected) => break,
            }
        }
    });
}

#[test]
fn mt_receiver_disconnect() {
    model(|| {
        let (src, sink) = channel::<i32>(1); //minimize the time loom takes.
        thread::spawn(|| drop(sink));
        loop {
            match src.try_send(0) {
                Ok(_) => thread::yield_now(),
                Err(TrySendError::Full(_)) => thread::yield_now(),
                Err(TrySendError::Disconnected(_)) => break,
            }
        }
    });
}

const CHANNEL_SIZE: u8 = 2;

#[test]
fn try_insert_try_remove() {
    let mut model = loom::model::Builder::new();
    model.max_threads = 2;
    model.check(|| {
        let (src, sink) = make_chan();
        try_insert(src);
        try_remove(sink);
    });
}

#[test]
fn try_insert_block_remove() {
    let mut model = loom::model::Builder::new();
    model.max_threads = 2;
    model.check(|| {
        let (src, sink) = make_chan();
        try_insert(src);
        block_remove(sink);
    });
}

#[test]
fn block_insert_try_remove() {
    let mut model = loom::model::Builder::new();
    model.max_threads = 2;
    model.check(|| {
        let (src, sink) = make_chan();
        block_insert(src);
        try_remove(sink);
    });
}

#[test]
fn block_insert_block_remove() {
    let mut model = loom::model::Builder::new();
    model.max_threads = 2;
    model.check(|| {
        let (src, sink) = make_chan();
        block_insert(src);
        block_remove(sink);
    });
}

fn make_chan() -> (Sender<u8>, Receiver<u8>) {
    channel::<u8>(CHANNEL_SIZE as usize)
}

fn try_insert(src: Sender<u8>) {
    thread::spawn(move || {
        for i in 0..=CHANNEL_SIZE {
            loop {
                match src.try_send(i) {
                    Ok(()) => break,
                    Err(TrySendError::Full(_)) => thread::yield_now(),
                    Err(TrySendError::Disconnected(_)) => panic!("Receiver dropped early."),
                }
            }
        }
    });
}

fn try_remove(sink: Receiver<u8>) {
    for i in 0..=CHANNEL_SIZE {
        loop {
            match sink.try_recv() {
                Ok(ret) => {
                    break assert_eq!(
                        ret, i,
                        "Data should be received in the same order as it was sent."
                    );
                }
                Err(TryRecvError::Empty) => thread::yield_now(),
                Err(TryRecvError::Disconnected) => {
                    panic!("Sender dropped before sending all data.")
                }
            }
        }
    }
}

fn block_insert(src: Sender<u8>) {
    thread::spawn(move || {
        for i in 0..=CHANNEL_SIZE {
            src.send(i).expect("Receiver dropped early");
        }
    });
}

fn block_remove(sink: Receiver<u8>) {
    for i in 0..=CHANNEL_SIZE {
        assert_eq!(
            i,
            sink.recv()
                .expect("Sender dropped before sending all data."),
            "Data should be received in the same order as it was sent."
        );
    }
}
}
