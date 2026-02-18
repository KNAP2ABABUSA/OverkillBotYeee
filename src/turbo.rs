use std::ffi::CString;



#[link(name = "turbo_engine")]
extern "C" {
    fn turbo_send_message_with_reply(
        token: *const std::os::raw::c_char,
        chat_id: *const std::os::raw::c_char,
        message: *const std::os::raw::c_char,
        proxy_url: *const std::os::raw::c_char,
        reply_to_message_id: i32,
    );
}
pub fn send_fast(token: &str, chat_id: &str, message: &str) -> bool {
    send_fast_with_reply(token, chat_id, message, "", None);
    true
}
pub fn send_fast_with_reply(
    token: &str,
    chat_id: &str,
    message: &str,
    proxy: &str,
    reply: Option<i32>,)
   {let c_token = CString::new(token).unwrap_or_else(|_| CString::new("").unwrap());
    let c_chat_id = CString::new(chat_id).unwrap_or_else(|_| CString::new("").unwrap());
    let c_message = CString::new(message).unwrap_or_else(|_| CString::new("").unwrap());
    let c_proxy = CString::new(proxy).unwrap_or_else(|_| CString::new("").unwrap());
    let reply_id = reply.unwrap_or(0); 
    
    unsafe {
        turbo_send_message_with_reply(
            c_token.as_ptr(),
            c_chat_id.as_ptr(),
            c_message.as_ptr(),
            c_proxy.as_ptr(),
            reply_id,);
        }}
//Точно не уверен, почему в моей головушке в четыре утра появилась идея вынести это в отдельный модуль, но уже поздно