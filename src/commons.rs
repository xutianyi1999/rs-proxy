use nanoid;

pub type Address = (String, u16);

pub fn create_channel_id() -> String {
  nanoid!(4)
}
