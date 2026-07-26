//! Shared library for the Sebenza server (`backend`) and CLI (`sebenza-cli`):
//! domain models + wire types, config load/persist, system adapters (git, tmux,
//! fs, docker, registries, agent session logs), and the sync orchestration
//! services. Server-only concerns (axum, WS, PTY, background loops) live in the
//! `backend` crate.
#![allow(dead_code)]

pub mod adapters;
pub mod config;
pub mod domain;
pub mod services;
pub mod util;
