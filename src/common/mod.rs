pub mod frozen;
pub mod location;
pub mod symbols;
pub mod unord;

#[macro_export]
macro_rules! unused {
    ($x:expr) => {
        match &($x) {
            _ => (),
        }
    };
}
