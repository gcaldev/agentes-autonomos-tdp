use crate::structs::user::User;

pub trait UsersRepository {
    fn find_user_by_username_and_password(&self, username: &str, password: &str) -> Option<User>;
}
