use crate::structs::user::User;
use ::servidor::commons::subject::subject_matches_token;

#[derive(Debug)]
/// Estructura para gestionar los permisos de suscripcion y publicacion sobre subjects
pub struct UserPermissions {
    subscribe: Vec<String>,
    publish: Vec<String>,
}
impl UserPermissions {
    pub fn from(user: &User) -> UserPermissions {
        UserPermissions {
            subscribe: user.get_subscribe(), // Clonamos para no tener referencias a los datos del model de nuestro repositorio de usuarios
            publish: user.get_publish(),
        }
    }

    /// Devuelve true si tiene permisos para suscribirse a ese topico. False en caso contrario.
    pub fn can_subscribe(&self, subject: &str) -> bool {
        self.subscribe
            .iter()
            .any(|token| subject_matches_token(token, subject))
    }

    /// Devuelve true si tiene permisos para publicar sobre ese topico. False en caso contrario.s
    pub fn can_publish(&self, subject: &str) -> bool {
        self.publish
            .iter()
            .any(|token| subject_matches_token(token, subject))
    }
}
