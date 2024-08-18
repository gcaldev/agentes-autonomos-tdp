pub fn subject_matches_token(token: &str, subject: &str) -> bool {
    let mut token_iter = token.split('.');
    let mut subject_iter = subject.split('.');

    loop {
        if let Some(token) = token_iter.next() {
            println!("expected token: {}", token);
            if token == ">" {
                return true;
            }
            if let Some(received_token) = subject_iter.next() {
                println!("received_token: {}", received_token);
                if token == "*" || token == received_token {
                    continue;
                }
                return false;
            }
            return false;
        }
        return subject_iter.next().is_none();
    }
}
