use crate::client::{
  constants::{LOCALHOST_URL, LOCALHOST_WS},
  utils::REGISTERED_USERS,
};
use client_api::Client;

mod client;
mod collab;
mod gotrue;
mod realtime;

pub fn client_api_client() -> Client {
  Client::from(reqwest::Client::new(), LOCALHOST_URL, LOCALHOST_WS)
}

pub async fn user_1_signed_in() -> Client {
  let user = &REGISTERED_USERS[0];
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL, LOCALHOST_WS);
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  c
}

pub async fn user_2_signed_in() -> Client {
  let user = &REGISTERED_USERS[1];
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL, LOCALHOST_WS);
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  c
}
