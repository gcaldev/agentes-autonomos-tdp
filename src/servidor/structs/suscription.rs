use servidor::commons::subject::subject_matches_token;
#[derive(Debug)]
/// Struct para contener los datos de una suscripcion a un topico determinado
/// sid: es el id dado por el cliente para la suscripcion a cierto subject
/// connection_id: Es el id del cliente al cual pertenece el sid
/// max_msg: Cantidad maxima de mensajes que puede recibir esta suscripcion para luego cancelarse/desuscribirse.
pub struct Suscription {
    pub connection_id: usize,
    pub subject: String,
    pub max_msgs: Option<usize>,
    pub sid: String,
}

impl Suscription {
    pub const fn new(
        connection_id: usize,
        subject: String,
        max_msgs: Option<usize>,
        sid: String,
    ) -> Suscription {
        Suscription {
            connection_id,
            subject,
            max_msgs,
            sid,
        }
    }

    /// Devuelve true si el subject pasado por parametro coincide con el guardado en el de la instancia
    pub fn matches(&self, subject: &str) -> bool {
        subject_matches_token(&self.subject, subject)
    }

    /// Decrementa los mensajes restantes que puede escuchar cierta suscripcion
    pub fn decrease_remaining_msgs(&mut self) {
        if let Some(max_msgs) = self.max_msgs.as_mut() {
            *max_msgs -= 1;
        }
    }
}
