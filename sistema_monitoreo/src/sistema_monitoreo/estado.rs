#[derive(Debug, Clone)]
/// Representa el estado de un incidente
pub enum Estado {
    //Resuelto,
    Pendiente,
    Cancelado,
    Activo,
}

impl Estado {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Estado::Pendiente => "pendiente",
            Estado::Cancelado => "cancelado",
            Estado::Activo => "activo",
        }
    }
}
