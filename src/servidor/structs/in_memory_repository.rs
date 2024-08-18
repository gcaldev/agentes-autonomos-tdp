use super::{user::User, users_repository::UsersRepository};
extern crate bcrypt;
use bcrypt::verify;

pub struct InMemoryUsersRepository {}

impl InMemoryUsersRepository {
    pub const fn new() -> Self {
        InMemoryUsersRepository {}
    }
}

impl UsersRepository for InMemoryUsersRepository {
    fn find_user_by_username_and_password(&self, username: &str, password: &str) -> Option<User> {
        let users = find_users();
        users.into_iter().find(|user| {
            user.get_user() == username && verify(password, user.get_pass()).unwrap_or(false)
        })
    }
}

/// funcion que se usa para encontrar los datos y credenciales de un usuario
fn find_users() -> Vec<User> {
    let users = vec![
        User::new(
            "a".to_string(),
            "$2b$12$9XmsdBtXYrDfZjKpsIvfp.WTIAnU/05JQcogfvb5OhiLGxKQXLWE2".to_string(),
            vec!["time".to_string(), "time.us".to_string()],
            vec!["time".to_string(), "time.us".to_string()],
            "a_log.txt",
        ),
        User::new(
            "b".to_string(),
            "$2b$12$ZRwLf1rHZJi4AIjj6f5YOeHGRIulIyfzjIr8DWjkDjg0lGHitNgIi".to_string(),
            vec![">".to_string()],
            vec![">".to_string(), "time.us".to_string()],
            "b_log.txt",
        ),
        User::new(
            "sismonitoreo".to_string(),
            "$2b$12$ZRwLf1rHZJi4AIjj6f5YOeHGRIulIyfzjIr8DWjkDjg0lGHitNgIi".to_string(),
            vec![">".to_string()],
            vec![">".to_string(), "time.us".to_string()],
            "sismonitoreo_log.txt",
        ),
        User::new(
            "centralcamaras".to_string(),
            "$2b$12$ZRwLf1rHZJi4AIjj6f5YOeHGRIulIyfzjIr8DWjkDjg0lGHitNgIi".to_string(),
            vec![">".to_string()],
            vec![">".to_string(), "time.us".to_string()],
            "centralcamaras_log.txt",
        ),
        User::new(
            "dronuno".to_string(),
            "$2b$12$ZRwLf1rHZJi4AIjj6f5YOeHGRIulIyfzjIr8DWjkDjg0lGHitNgIi".to_string(),
            vec![">".to_string()],
            vec![">".to_string(), "time.us".to_string()],
            "dronuno_log.txt",
        ),
    ];
    // pass prueba123
    users
}
