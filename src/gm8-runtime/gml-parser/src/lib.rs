#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
extern crate alloc;

pub mod ast;
pub mod lexer;
pub mod token;
