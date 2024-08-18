#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use camara::camara::central_de_camaras::CentralDeCamaras;
use eframe::egui;
fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Central de Camaras",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    )
}

struct Form {
    latitud: String,
    longitud: String,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            latitud: "".to_string(),
            longitud: "".to_string(),
        }
    }
}

struct MyApp {
    cliente: Option<CentralDeCamaras>,
    lista: Vec<Camara>,
    form: Form,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            lista: vec![],
            form: Form::default(),
            cliente: match CentralDeCamaras::new() {
                Ok(c) => {
                    Some(c)
                },
                Err(e) => {
                    println!("Error al crear central de camaras {}", e);
                    None
                },
            },
        }
    }
}

impl MyApp {
    fn eliminar_camara(&mut self, id: usize) {
        if let Some(cliente) = &mut self.cliente{
            if cliente.eliminar_camara(&id).is_ok() {
                self.lista.retain(|cam| cam.id != id);
            }
        }
    }

    fn agregar_camara(&mut self) {
        if let Ok(longitud) = self.form.longitud.parse::<f64>() {
            if let Ok(latitud) = self.form.latitud.parse::<f64>() {
                if let Some(cliente) = &mut self.cliente{
                    if let Ok(id) = cliente.nueva_camara(latitud, longitud) {
                        self.lista
                            .push(Camara::new(*id, longitud, latitud, "activo"));
                        self.limpiar_form();
                    }
                }
            }
        }
    }

    fn limpiar_form(&mut self) {
        self.form.latitud = "".to_string();
        self.form.longitud = "".to_string();
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut to_delete: Option<usize> = None;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Central de Camaras");
            egui::Grid::new("Camaras").striped(true).show(ui, |ui| {
                ui.label("ID");
                ui.label("Longitud");
                ui.label("Latitud");
                ui.label("Borrar");
                ui.end_row();
                for cam in self.lista.iter_mut() {
                    ui.label(cam.id.to_string());
                    ui.label(cam.longitud.to_string());
                    ui.label(cam.latitud.to_string());
                    if ui.button("Eliminar").clicked() {
                        to_delete = Some(cam.id);
                    }
                    ui.end_row();
                }
            });

            if let Some(id) = to_delete {
                self.eliminar_camara(id);
            }
            ui.separator();
            ui.label("Nueva Camara");
            ui.vertical(|ui| {
                ui.label("Longitud:");
                ui.text_edit_singleline(&mut self.form.longitud);
            });

            ui.vertical(|ui| {
                ui.label("Latitud:");
                ui.text_edit_singleline(&mut self.form.latitud);
            });

            ui.horizontal(|ui| {
                if ui.button("limpiar").clicked() {
                    self.limpiar_form();
                }
                if ui.button("Agregar").clicked() {
                    self.agregar_camara();
                }
            });
        });
    }
}

pub struct Camara {
    pub id: usize,
    pub longitud: f64,
    pub latitud: f64,
    pub estado: String,
}

impl Camara {
    pub fn new(id: usize, longitud: f64, latitud: f64, estado: &str) -> Self {
        Self {
            id,
            longitud,
            latitud,
            estado: estado.to_string(),
        }
    }
}
