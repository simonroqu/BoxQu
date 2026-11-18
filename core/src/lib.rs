//! BoxQu ohne Oberfläche: alles, was nicht an GTK hängt. Die GTK-App (`../src`) nutzt es, die Android-App über
//! `ffi/` (Ordner vorher mit `qu_core::app::set_dirs` setzen).

pub mod aniskip;
pub mod dash;
pub mod lists;
pub mod moviebox;
pub mod progress;
pub mod source;
