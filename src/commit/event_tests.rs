use super::*;
use std::future::Future;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Wake};

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn channel() -> (EventSender, Events) {
    let (send, receive) = mpsc::sync_channel(16);
    let notification = Arc::new(Notification::default());
    (
        EventSender {
            send: Some(send),
            notification: notification.clone(),
        },
        Events {
            receive,
            control: Weak::new(),
            notification,
        },
    )
}

#[test]
fn async_events_wake_for_output_and_disconnect_without_blocking() {
    let (sender, mut events) = channel();
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut context = Context::from_waker(&waker);
    let control = Control {
        child: Mutex::new(None),
        abandoned: AtomicBool::new(false),
    };
    {
        let mut next = std::pin::pin!(events.next());
        assert!(next.as_mut().poll(&mut context).is_pending());
        assert!(send_event(
            &sender,
            &control,
            Event::Output(vec![0, 255, 1])
        ));
        assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
        assert!(matches!(next.as_mut().poll(&mut context),
            Poll::Ready(Ok(Event::Output(bytes))) if bytes == [0, 255, 1]));
    }
    let mut next = std::pin::pin!(events.next());
    assert!(next.as_mut().poll(&mut context).is_pending());
    drop(sender);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 2);
    assert!(matches!(
        next.as_mut().poll(&mut context),
        Poll::Ready(Err(_))
    ));
}

#[test]
fn async_events_keep_queue_order_when_output_precedes_polling() {
    let (sender, mut events) = channel();
    let control = Control {
        child: Mutex::new(None),
        abandoned: AtomicBool::new(false),
    };
    assert!(send_event(&sender, &control, Event::Output(vec![1])));
    assert!(send_event(&sender, &control, Event::Output(vec![2])));
    drop(sender);
    let waker = Waker::from(Arc::new(WakeCount::default()));
    let mut context = Context::from_waker(&waker);
    for expected in [1, 2] {
        let mut next = std::pin::pin!(events.next());
        assert!(matches!(next.as_mut().poll(&mut context),
            Poll::Ready(Ok(Event::Output(bytes))) if bytes == [expected]));
    }
    let mut next = std::pin::pin!(events.next());
    assert!(matches!(
        next.as_mut().poll(&mut context),
        Poll::Ready(Err(_))
    ));
}
