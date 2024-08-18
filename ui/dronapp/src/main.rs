use dron::dron::droncliente::Droncliente;
use eframe::egui;

#[derive(Default)]
pub struct DronUI {
    id: String,
    latitud: String,
    longitud: String,
    alcance: String,
    dron_cliente: Option<Droncliente>,
    inicio: bool,
    bateria: String,
}

// impl Default for DronUI {
//     fn default() -> Self {
//         Self {
//             id: String::new(),
//             latitud: String::new(),
//             longitud: String::new(),
//             alcance: String::new(),
//             dron_cliente: None,
//             inicio: false,
//             bateria: String::new(),
//         }
//     }
// }

impl eframe::App for DronUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.inicio {
                ui.label("Ingrese los datos del nuevo dron");

                ui.horizontal(|ui| {
                    ui.label("ID:");
                    ui.text_edit_singleline(&mut self.id);
                });
                ui.horizontal(|ui| {
                    ui.label("Longitud:");
                    ui.text_edit_singleline(&mut self.longitud);
                });
                ui.horizontal(|ui| {
                    ui.label("Latitud:");
                    ui.text_edit_singleline(&mut self.latitud);
                });

                ui.horizontal(|ui| {
                    ui.label("Alcance:");
                    ui.text_edit_singleline(&mut self.alcance);
                });

                ui.horizontal(|ui| {
                    ui.label("Bateria:");
                    ui.text_edit_singleline(&mut self.bateria);
                });

                if ui.button("Crear Dron").clicked() {
                    if let Ok(id) = self.id.parse::<i32>() {
                        if let Ok(latitud) = self.latitud.parse::<f64>() {
                            if let Ok(longitud) = self.longitud.parse::<f64>() {
                                if let Ok(alcance) = self.alcance.parse::<f64>() {
                                    if let Ok(bateria) = self.bateria.parse::<f64>() {
                                        if let Ok(dron_cl) = Droncliente::new(
                                            id, latitud, longitud, alcance, bateria,
                                        ) {
                                            self.dron_cliente = Some(dron_cl)
                                        } else {
                                            println!("Error al crear el Dron Cliente");
                                        }

                                        self.inicio = true;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                ui.label("Datos del dron:");

                if let Some(lock_dron_cliente) = &self.dron_cliente {
                    if let Ok(dron) = lock_dron_cliente.dron.lock() {
                        ui.horizontal(|ui| {
                            ui.label(format!("ID: {}", dron.get_id()));
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Estado: {}", dron.get_estado()));
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Latitud: {}", dron.get_latitud()));
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Longitud: {}", dron.get_longitud()));
                        });
                        ui.horizontal(|ui| {
                            ui.label(format!("Batería: {}", dron.get_bateria()));
                        });
                    }
                }
            }
        });
    }
}

fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([500.0, 300.0]),
        ..Default::default()
    };

    let _ = eframe::run_native("Dron", options, Box::new(|_cc| Box::new(DronUI::default())));
}
