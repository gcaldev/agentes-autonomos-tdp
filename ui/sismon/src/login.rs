#[allow(clippy::module_inception)]
pub mod login {
    use sistema_monitoreo::sistema_monitoreo::SistemaDeMonitoreo;
    pub struct Login {
        pub username: String,
        pub password: String,
        pub show_incorrect: bool,
        pub login_successful: bool,
    }

    impl Default for Login {
        fn default() -> Self {
            Self {
                username: "".to_string(),
                password: "".to_string(),
                show_incorrect: false,
                login_successful: false,
            }
        }
    }

    impl Login {
        pub fn render(
            &mut self,
            ui: &mut egui::Ui,
            sistema_monitoreo: &mut Option<SistemaDeMonitoreo>,
        ) {
            ui.heading("Iniciar Sesión");

            ui.vertical(|ui| {
                ui.label("Usuario:");
                if ui.text_edit_singleline(&mut self.username).changed() {
                    self.show_incorrect = false;
                }
            });

            ui.vertical(|ui| {
                ui.label("Contraseña:");
                if ui.text_edit_singleline(&mut self.password).changed() {
                    self.show_incorrect = false;
                }
            });

            if self.show_incorrect {
                ui.colored_label(egui::Color32::RED, "Usuario o contraseña incorrectos");
            }

            if ui.button("Iniciar Sesión").clicked() {
                match SistemaDeMonitoreo::new(self.username.clone(), self.password.clone()) {
                    Ok(sistema) => {
                        *sistema_monitoreo = Some(sistema);
                        self.login_successful = true;
                    }
                    Err(_) => {
                        self.show_incorrect = true;
                    }
                }
            }
        }
    }
}
