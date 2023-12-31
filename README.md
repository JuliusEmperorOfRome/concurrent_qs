# concurrent_qs

A rust crate that aims to provide access to common queues used in concurrent programming.
Currently, only SPSC queues are provided.

## Usage

All queues are enabled with a `{type}-{bounded|unbounded}` feature and are placed
in `{type}::{bounded|unbounded}`. There are also `{type}-all` and `full` features,
enabling all `{type}` queues and the full library respectively.

For example, to use a bounded SPSC queue, you would write something like this.

```toml
[dependencies]
concurrent_qs = ["spsc-bounded"]
```

```rust,no_run
use concurrent_qs::spsc::bounded;
use concurrent_qs::error::*;

fn main() {
    let(tx, rx) = bounded::channel::<&'static str>(1);

    std::thread::spawn(move || {
        tx.send("Hello").expect("Fails only after 'rx' is dropped.");
        tx.send(", ").unwrap();
        tx.send("World").unwrap();
        // Do not use try_send this way. If you **need** to send the value
        // to proceed, use send instead.
        let mut send: &'static str = "!";
        loop {
            send = match tx.try_send(send) {
                Ok(()) => return,
                Err(TrySendError::Full(fail)) => fail,
                Err(TrySendError::Disconnected(_)) => unreachable!(),
            }
        }
    });

    let mut result = String::new();
    result.push_str(
        loop {
            // Do not use try_recv this way. If you **need** to
            // get a value to proceed, use recv instead.
            match rx.try_recv() {
                Ok(s) => break s,
                Err(TryRecvError::Empty) => std::hint::spin_loop(),
                Err(TryRecvError::Disconnected) => {
                    unreachable!("Disconnect only happens after receiving all data")
                }
            }
        }
    );
    while let Ok(s) = rx.recv() {
        result.push_str(s);
    }

    assert_eq!(result.as_str(), "Hello, World!");
}
```

# Features

- full &mdash; enables all queues in the crate.
- spsc-all &mdash; enables **\[un\]bounded::spsc** queues.
- spsc-bounded &mdash; enables **the bounded::spsc** queue.
- spsc-unbounded &mdash; enables **the unbounded::spsc** queue.