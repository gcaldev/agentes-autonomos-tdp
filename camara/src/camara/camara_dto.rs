use super::{camara::Camara, estado_camara::EstadoCamara, posicion::Posicion};

/// Estructura que contiene los datos que se informaran al sistema de monitoreo
pub struct CamaraDto {
    pub id: usize,
    pub estado: EstadoCamara,
    pub posicion: Posicion,
}

impl CamaraDto {
    pub fn from(camara: &Camara) -> Self {
        CamaraDto {
            id: camara.id,
            estado: camara.get_estado(),
            posicion: camara.get_posision(),
        }
    }
    
    pub fn get_estado(&self) -> EstadoCamara {
        self.estado.clone()
    }

    pub fn get_posision(&self) -> Posicion {
        self.posicion.clone()
    }

    const fn _get_id(&self) -> &usize {
        &self.id
    }
}