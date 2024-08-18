#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use egui::{CentralPanel, Context};
use incidencias::incidencias::{FormIncidencia, Incidencia, Incidencias};
use login::login::Login;
use mapa::mapa::Mapa;
use sistema_monitoreo::sistema_monitoreo::SistemaDeMonitoreo;
use sistema_monitoreo::sistema_monitoreo::incidente::Incidente;

mod incidencias;
mod login;
mod mapa;
fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([400.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Central de Monitoreo",
        options,
        Box::new(|cc| Box::new(MyApp::new(cc.egui_ctx.clone()))),
    )
}

struct MyApp {
    show_login: bool,
    show_incidencias: bool,
    login: Login,
    login_successful: bool,
    incidencias: Vec<Incidencia>,
    show_form: bool,
    form_incidencia: FormIncidencia,
    sistema_monitoreo: Option<SistemaDeMonitoreo>,
    incidencias_render: Incidencias,
    show_map: bool,
    mapa: Mapa,
}

impl MyApp {
    fn new(egui_ctx: Context) -> Self {
        Self {
            show_login: true,
            login: Login::default(),
            login_successful: false,
            incidencias: vec![],
            show_form: false,
            form_incidencia: FormIncidencia::default(),
            show_incidencias: true,
            sistema_monitoreo: None,
            incidencias_render: Incidencias::default(),
            show_map: false,
            mapa: Mapa::new(egui_ctx),
        }
    }

    fn nueva_incidencia(&mut self) {
        let longitud: f64 = match self.form_incidencia.longitud.parse() {
            Ok(long) => long,
            Err(e) => {
                println!("Error en al parsear la longitud: {}", e);
                return;
            },
        };

        let latitud: f64 = match self.form_incidencia.latitud.parse(){
            Ok(lat) => lat,
            Err(e) => {
                println!("Error en al parsear la latitud: {}", e);
                return;
            },
        };

        if let Some(sistema) = &mut self.sistema_monitoreo {
            if let Ok(id) = sistema.nuevo_incidente(latitud, longitud, 10) {
                let etiqueta = &self.form_incidencia.etiqueta;
                let tiempo = "2024-05-12 17:00"; //usar chrono
                 
                let nueva_incidencia = Incidencia::new(id, etiqueta, longitud, latitud, tiempo);
                self.incidencias.push(nueva_incidencia);
                // Limpia el formulario
                self.form_incidencia = FormIncidencia::default();
            }
        }
    }

    fn actualizar_incidencias(&mut self) {
        if let Some(sistema) = &mut self.sistema_monitoreo {
            if let Ok(incidentes) = sistema.obtener_info_incidentes() {
                println!("incidentes {:?}", incidentes);

                let new_incidentes: Vec<Incidente> = incidentes
                    .iter()
                    .filter(|inc| inc.get_estado().as_str() != "cancelado")
                    .cloned()
                    .collect();

                let new_ids: Vec<String> = new_incidentes
                    .iter()
                    .map(|inc| inc.get_id().to_string())
                    .collect();

                self.incidencias.retain(|inc| new_ids.contains(&inc.id));

                for new_inc in new_incidentes {
                    if !self
                        .incidencias
                        .iter()
                        .any(|inc| inc.id == new_inc.get_id())
                    {
                        let etiqueta = "Incidente";
                        let posicion = new_inc.get_posision();

                        let tiempo = "2024-05-12 17:00";

                        let nueva_incidencia = Incidencia::new(
                            new_inc.get_id().to_string(),
                            etiqueta,
                            posicion.long,
                            posicion.lat,
                            tiempo,
                        );
                        self.incidencias.push(nueva_incidencia);
                    }
                }
            }
        }
    }

    
    fn eliminar_incidencia(&mut self, id: String) {
        if let Some(sistema) = &mut self.sistema_monitoreo {
            let _ = sistema.eliminar_incidente(&id);
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut to_delete: Option<String> = None;

        CentralPanel::default().show(ctx, |ui| {
            if self.show_login {
                ui.heading("Bienvenido a la Central de Monitoreo");

                self.login.render(ui, &mut self.sistema_monitoreo);

                if self.login.login_successful {
                    self.show_login = false;
                    self.login_successful = true;
                }
            }

            if self.login_successful {
                if self.show_incidencias {
                    ui.label("Bienvenido, admin");
                    ui.separator();

                    self.actualizar_incidencias();

                    self.incidencias_render
                        .render(ui, & self.incidencias, |id| {
                            to_delete = Some(id.to_string());
                        });

                    ui.horizontal(|ui| {
                        if ui.button("Cargar Incidente").clicked() {
                            self.show_form = true;
                            self.show_incidencias = false;
                        }

                        if ui.button("Ver Mapa").clicked() {
                            self.show_map = true;
                            self.show_incidencias = false;
                        }
                    });
                }

                if let Some(id) = to_delete {
                    self.eliminar_incidencia(id);
                }

                if self.show_form {
                    self.form_incidencia.submit_clicked = false;
                    self.form_incidencia.render(ui);

                    if self.form_incidencia.submit_clicked {
                        self.nueva_incidencia();
                        self.show_form = false;
                        self.show_incidencias = true;
                    }
                }

                if self.show_map {
                    ui.horizontal(|ui| {
                        ui.heading("Mapa");
                        if ui.button("Volver").clicked() {
                            self.show_map = false;
                            self.show_incidencias = true;
                        }
                    });

                    if let Some(sistema) = &mut self.sistema_monitoreo {
                        if let Ok(camaras) = sistema.obtener_info_camaras() {
                            let drones = sistema.obtener_info_drones();
                            self.actualizar_incidencias();
                            self.mapa
                                .render(ui, self.incidencias.clone(), camaras, drones, ctx);
                        }
                    }
                }
            }
        });
    }
}
