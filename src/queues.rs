use once_cell::sync::Lazy;
use tokio::sync::mpsc;

pub static EXCHANGE_QUEUE: Lazy<(mpsc::Sender<()>, mpsc::Receiver<()>)> =
    Lazy::new(|| mpsc::channel(32));
