#[allow(clippy::module_inception)]
pub mod mapa {
    use crate::incidencias::incidencias::Incidencia;
    use egui::{
        Color32, Context, Painter, Pos2, Rect, Response, TextureHandle, TextureOptions, Vec2,
    };
    use image::ImageError;
    use sistema_monitoreo::sistema_monitoreo::camara::Camara;
    use sistema_monitoreo::sistema_monitoreo::dron::Dron;
    use walkers::{sources::OpenStreetMap, Map, MapMemory, Plugin, Position, Projector, Tiles};

    pub struct Eventos {
        incidencias: Vec<Incidencia>,
        camaras: Vec<Camara>,
        drones: Vec<Dron>,
        camara_texture: TextureHandle,
        cam1_texture: TextureHandle,
        cam2_texture: TextureHandle,
        dron_texture: TextureHandle,
    }

    fn load_texture(
        bytes: &[u8],
        name: &str,
        egui_ctx: &egui::Context,
    ) -> Result<TextureHandle, ImageError> {
        let image = image::load_from_memory(bytes)?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let pixels = image.into_raw();
        let texture_handler = egui_ctx.load_texture(
            name,
            egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
            TextureOptions::default(),
        );
        Ok(texture_handler)
    }

    impl Eventos {
        pub fn new(
            incidencias: Vec<Incidencia>,
            camaras: Vec<Camara>,
            drones: Vec<Dron>,
            egui_ctx: &egui::Context,
        ) -> Result<Self, ImageError> {
            let camara_image = include_bytes!("../imagenes/incidente.png");
            let camara_texture = load_texture(camara_image, "camara", egui_ctx)?;

            let cam1_image = include_bytes!("../imagenes/camgrabando.png");
            let cam1_texture = load_texture(cam1_image, "cam1", egui_ctx)?;

            let cam2_image = include_bytes!("../imagenes/camahorro.png");
            let cam2_texture = load_texture(cam2_image, "cam2", egui_ctx)?;

            let dron_image = include_bytes!("../imagenes/dron.png");
            let dron_texture = load_texture(dron_image, "dron", egui_ctx)?;

            Ok(Self {
                incidencias,
                camaras,
                drones,
                camara_texture,
                cam1_texture,
                cam2_texture,
                dron_texture,
            })
        }
    }

    impl Plugin for Eventos {
        fn run(&mut self, _response: &Response, painter: Painter, projector: &Projector) {
            for incidencia in &self.incidencias {
                let position = Position::from_lon_lat(incidencia.longitud, incidencia.latitud);
                let screen_pos = projector.project(position);

                let screen_pos = Pos2::new(screen_pos.x, screen_pos.y);

                let rect = Rect::from_center_size(screen_pos, Vec2::new(28.0, 28.0));
                let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                painter.image(self.camara_texture.id(), rect, uv, Color32::WHITE);

                let text_pos = Pos2::new(screen_pos.x, screen_pos.y + 12.0);
                painter.text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    format!("{} (ID: {})", incidencia.etiqueta, incidencia.id),
                    egui::FontId::default(),
                    Color32::BLACK,
                );
            }

            for camara in &self.camaras {
                let position = Position::from_lon_lat(camara.posicion.long, camara.posicion.lat);
                let screen_pos = projector.project(position);

                let screen_pos = Pos2::new(screen_pos.x, screen_pos.y);

                let texture = if camara.estado == "ahorro" {
                    &self.cam2_texture
                } else {
                    &self.cam1_texture
                };

                let rect = Rect::from_center_size(screen_pos, Vec2::new(35.0, 35.0));
                let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                painter.image(texture.id(), rect, uv, Color32::WHITE);

                let text_pos = Pos2::new(screen_pos.x, screen_pos.y + 12.0);
                painter.text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    format!("Cámara {}", camara.id),
                    egui::FontId::default(),
                    Color32::BLACK,
                );
            }

            for dron in &self.drones {
                let position = Position::from_lon_lat(dron.posicion.long, dron.posicion.lat);
                let screen_pos = projector.project(position);

                let screen_pos = Pos2::new(screen_pos.x, screen_pos.y);

                let rect = Rect::from_center_size(screen_pos, Vec2::new(35.0, 35.0));
                let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                painter.image(self.dron_texture.id(), rect, uv, Color32::WHITE);

                let text_pos = Pos2::new(screen_pos.x, screen_pos.y + 15.0);
                painter.text(
                    text_pos,
                    egui::Align2::CENTER_CENTER,
                    format!("Dron {}\nBatería: {:.2}%", dron.id, dron.bateria),
                    egui::FontId::default(),
                    Color32::BLACK,
                );
            }
        }
    }

    pub struct Mapa {
        tiles: Tiles,
        map_memory: MapMemory,
    }

    impl Mapa {
        pub fn new(egui_ctx: Context) -> Self {
            Self {
                tiles: Tiles::new(OpenStreetMap, egui_ctx),
                map_memory: MapMemory::default(),
            }
        }

        pub fn render(
            &mut self,
            ui: &mut egui::Ui,
            incidencias: Vec<Incidencia>,
            camaras: Vec<Camara>,
            drones: Vec<Dron>,
            egui_ctx: &Context,
        ) {
            if let Ok(ev) = Eventos::new(incidencias, camaras, drones, egui_ctx) {
                ui.add(
                    Map::new(
                        Some(&mut self.tiles),
                        &mut self.map_memory,
                        Position::from_lon_lat(-58.3734, -34.6191),
                    )
                    .with_plugin(ev),
                );
            }
        }
    }
}
