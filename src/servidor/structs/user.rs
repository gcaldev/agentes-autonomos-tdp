#[derive(Debug)]
/// Estructura para contener los datos un usuario y sus credenciales
pub struct User {
    user: String,
    pass: String,
    subscribe: Vec<String>,
    publish: Vec<String>,
    message_log: String,
}

impl User {
    pub fn new(
        user: String,
        pass: String,
        subscribe: Vec<String>,
        publish: Vec<String>,
        message_log: &str,
    ) -> Self {
        User {
            user,
            pass,
            subscribe,
            publish,
            message_log: message_log.to_string(),
        }
    }

    pub fn get_user(&self) -> &str {
        &self.user
    }

    pub fn get_pass(&self) -> &str {
        &self.pass
    }

    pub fn get_subscribe(&self) -> Vec<String> {
        self.subscribe.clone()
    }

    pub fn get_publish(&self) -> Vec<String> {
        self.publish.clone()
    }

    pub fn get_log(&self) -> &str {
        &self.message_log
    }
}
