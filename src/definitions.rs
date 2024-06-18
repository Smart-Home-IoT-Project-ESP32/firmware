extern crate alloc;
use alloc::vec;
use messages::dynamic::DynamicAccess;

messages::message_structs!(Message, "definitions");

pub fn set_message_device_id(message: &mut Message, device_id: u8) -> Result<()> {
    // get id of message
    let id = message.get_id();
    // get device_id field of message
    let device_id_field = database()
        .get(&id.into())
        .unwrap()
        .fields
        .iter()
        .find(|field| field.name == "Device ID")
        .ok_or(Error::FrameIsNotMessage)?;
    // set device_id field of message
    message.set_field(device_id_field, AnyField::U64(device_id.into()))
}
