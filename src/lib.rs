#![no_std]
#![feature(impl_trait_in_assoc_type)]

pub mod constants;
pub mod http;
pub mod models;
pub mod network;
pub mod utils;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
