use std::{
    sync::mpsc::{self, Sender},
    thread::{self, JoinHandle},
};

use super::errors::NatsClientError;
//                         payload      headers        reply-to
pub type SuscriptionInfo = (String, Option<String>, Option<String>);
pub type SuscriptionSender = Sender<SuscriptionInfo>;

#[derive(Debug)]
pub struct Consumer {
    name: String,
    stream_name: String,
}
impl Consumer {
    pub fn new(stream_name: &str, name: &str) -> Self {
        Consumer {
            name: name.to_string(),
            stream_name: stream_name.to_string(),
        }
    }

    pub fn to_command(&self, ack_id: &str) -> String {
        format!("$JS.ACK.{}.{}.{}\r\n", self.stream_name, self.name, ack_id)
    }
}

#[derive(Debug)]
pub struct SuscriptionHandler {
    pub id: usize,
    pub subject: String,
    pub tx: SuscriptionSender,
    pub max_msgs: Option<usize>,
    pub handler: JoinHandle<()>,
    pub consumer: Option<Consumer>,
}

impl SuscriptionHandler {
    pub fn new<F: Fn(SuscriptionInfo) + Send + 'static>(
        id: usize,
        subject: &str,
        f: F,
        consumer: Option<Consumer>,
    ) -> Result<Self, NatsClientError> {
        let invalid_wildcard = subject.split('.').any(|x| {
            if x.contains('*') && x.len() > 1 {
                return true;
            }
            false
        });

        let invalid_full_wildcard = (subject.contains('>') && !subject.ends_with('>'))
            || subject.split('.').last().map_or(true, |last_token| {
                last_token.ends_with('>') && last_token.len() > 1
            });

        if invalid_full_wildcard || invalid_wildcard {
            return Err(NatsClientError::InvalidSubject);
        }

        let builder = thread::Builder::new().name(format!("thread-{}", id));
        let (tx, rx) = mpsc::channel::<SuscriptionInfo>();

        let suscription_handler = builder
            .spawn(move || {
                while let Ok(payload) = rx.recv() {
                    println!("SUB THREAD |{}|", payload.0);
                    f(payload);
                }
            })
            .map_err(|_| {
                NatsClientError::InternalError("Error al crear el suscription handler".to_string())
            })?;

        Ok(SuscriptionHandler {
            id,
            subject: subject.to_string(),
            tx,
            handler: suscription_handler,
            consumer,
            max_msgs: None,
        })
    }

    pub fn matches(&self, subject: &str) -> bool {
        let mut expected_sub_iter = self.subject.split('.');
        let mut received_sub_iter = subject.split('.');

        loop {
            if let Some(token) = expected_sub_iter.next() {
                println!("expected token: {}", token);
                if token == ">" {
                    return true;
                }
                if let Some(received_token) = received_sub_iter.next() {
                    println!("received_token: {}", received_token);
                    if token == "*" || token == received_token {
                        continue;
                    }
                    return false;
                }
                return false;
            }
            return received_sub_iter.next().is_none();
        }
    }
}

#[cfg(test)]
mod test_suscription_rules {
    use crate::cliente::{errors::NatsClientError, suscriptor_handler::SuscriptionHandler};

    #[test]
    fn test01_matches_exact() {
        let sub = SuscriptionHandler::new(0, "INCIDENTES", |_| {}, None).unwrap();
        assert!(sub.matches("INCIDENTES"));
    }

    #[test]
    fn test02_case_sensitive() {
        let sub = SuscriptionHandler::new(0, "inCidentEs", |_| {}, None).unwrap();

        assert!(!sub.matches("INCIDENTES"));
    }

    #[test]
    fn test03_multiple_tokens() {
        let sub = SuscriptionHandler::new(0, "foo.baz", |_| {}, None).unwrap();

        assert!(sub.matches("foo.baz"));
    }

    #[test]
    fn test04_wildcard_ok() {
        let sub = SuscriptionHandler::new(0, "foo.*.baz", |_| {}, None).unwrap();

        assert!(sub.matches("foo.any.baz"));
    }

    #[test]
    fn test05_fullwildcard_ok() {
        let sub = SuscriptionHandler::new(0, "foo.>", |_| {}, None).unwrap();

        assert!(sub.matches("foo.bar.baz.1"));
    }

    #[test]
    fn test06_invalid_wildcard() {
        let sub_handler = SuscriptionHandler::new(0, "foo*.bar", |_| {}, None);

        assert!(matches!(
            sub_handler.unwrap_err(),
            NatsClientError::InvalidSubject
        ));
    }

    #[test]
    fn test07_invalid_fullwildcard() {
        let sub_handler = SuscriptionHandler::new(0, "foo>", |_| {}, None);

        assert!(matches!(
            sub_handler.unwrap_err(),
            NatsClientError::InvalidSubject
        ));
    }

    #[test]
    fn test08_fullwildcard_in_the_middle() {
        let sub_handler = SuscriptionHandler::new(0, "foo.>.aca", |_| {}, None);

        assert!(matches!(
            sub_handler.unwrap_err(),
            NatsClientError::InvalidSubject
        ));
    }
}
