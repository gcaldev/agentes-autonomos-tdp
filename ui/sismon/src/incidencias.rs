#[allow(clippy::module_inception)]
pub mod incidencias {

    #[derive(Debug, Clone)]
    pub struct Incidencia {
        pub id: String,
        pub etiqueta: String,
        pub longitud: f64,
        pub latitud: f64,
        pub tiempo: String,
    }
    impl Incidencia {
        pub fn new(id: String, etiqueta: &str, longitud: f64, latitud: f64, tiempo: &str) -> Self {
            Self {
                id,
                etiqueta: etiqueta.to_string(),
                longitud,
                latitud,
                tiempo: tiempo.to_string(),
            }
        }
    }

    pub struct Incidencias {
        title: String,
    }

    impl Default for Incidencias {
        fn default() -> Self {
            Self {
                title: "Incidentes".to_string(),
            }
        }
    }

    impl Incidencias {
        pub fn render(
            &self,
            ui: &mut egui::Ui,
            lista: &[Incidencia],
            mut to_delete: impl FnMut(&str),
        ) {
            ui.heading(&self.title);
            egui::Grid::new(&self.title).striped(true).show(ui, |ui| {
                ui.label("ID");
                ui.label("Etiqueta");
                ui.label("Longitud");
                ui.label("Latitud");
                ui.label("Tiempo");
                ui.label("Acciones");
                ui.end_row();

                for incidencia in lista {
                    ui.label(incidencia.id.to_string());
                    ui.label(&incidencia.etiqueta);
                    ui.label(incidencia.longitud.to_string());
                    ui.label(incidencia.latitud.to_string());
                    ui.label(&incidencia.tiempo);
                    if ui.button("eliminar").clicked() {
                        to_delete(&incidencia.id);
                    }
                    ui.end_row();
                }
            });
        }
    }

    pub struct FormIncidencia {
        pub etiqueta: String,
        pub longitud: String,
        pub latitud: String,
        pub descripcion: String,
        pub submit_clicked: bool,
    }

    impl Default for FormIncidencia {
        fn default() -> Self {
            Self {
                etiqueta: "".to_string(),
                longitud: "".to_string(),
                latitud: "".to_string(),
                descripcion: "".to_string(),
                submit_clicked: false,
            }
        }
    }

    impl FormIncidencia {
        pub fn render(&mut self, ui: &mut egui::Ui) {
            ui.heading("Cargar Incidente");

            ui.vertical(|ui| {
                ui.label("Etiqueta:");
                ui.text_edit_singleline(&mut self.etiqueta);
            });

            ui.vertical(|ui| {
                ui.label("Longitud:");
                ui.text_edit_singleline(&mut self.longitud);
            });

            ui.vertical(|ui| {
                ui.label("Latitud:");
                ui.text_edit_singleline(&mut self.latitud);
            });

            ui.vertical(|ui| {
                ui.label("Descripción:");
                ui.text_edit_multiline(&mut self.descripcion);
            });

            ui.horizontal(|ui| {
                if ui.button("Volver").clicked() {
                    self.submit_clicked = true;
                }
                if ui.button("Guardar Incidente").clicked() {
                    self.submit_clicked = true;
                }
            });
        }
    }
}
