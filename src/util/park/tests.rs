#[cfg(test)]
use super::Parker;
cfg_not_loom! {

#[test]
fn test_two_threads() {
    static PARKER: Parker = Parker::new();
    std::thread::spawn(|| PARKER.unpark());
    unsafe { PARKER.park() };
}

#[test]
fn test_one_thread() {
    let parker = Parker::new();
    parker.unpark();
    unsafe { parker.park() };
}

}

cfg_loom! {

#[test]
fn test_two_threads() {
    loom::model::model(|| {
        use std::sync::Arc;
        let parker = Arc::new(Parker::new());
        let cloned = parker.clone();
        loom::thread::spawn(move || {
            cloned.unpark();
        });
        unsafe { parker.park() };
    });
}

#[test]
fn test_one_thread() {
    loom::model::model(|| {
        let parker = Parker::new();
        parker.unpark();
        unsafe { parker.park() };
    });
}

}
