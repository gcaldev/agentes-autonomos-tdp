use super::{errors::NatsClientError, suscriptor_handler::SuscriptionInfo};
use std::io::Error;

pub trait INatsClient {
    fn create_stream(&mut self, subject: &str, name: &str) -> Result<(), NatsClientError>;

    fn create_and_consume(
        &mut self,
        stream_name: &str,
        consumer_name: &str,
        delivery_subject: &str,
        f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
    ) -> Result<usize, NatsClientError>;

    fn publish(
        &mut self,
        subject: &str,
        payload: Option<&str>,
        reply_subject: Option<&str>,
    ) -> Result<(), NatsClientError>;

    fn hpublish(
        &mut self,
        subject: &str,
        headers: &str,
        payload: &str,
        reply_subject: Option<&str>,
    ) -> Result<(), Error>;

    fn subscribe(
        &mut self,
        subject: &str,
        f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
    ) -> Result<usize, NatsClientError>;

    fn unsubscribe(
        &mut self,
        subject_id: usize,
        max_msgs: Option<usize>,
    ) -> Result<(), NatsClientError>;
}
