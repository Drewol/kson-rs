# KSON-rs
Rust implementation of the latest SDVX simulator chart format.

## Projects in this repo
The latest builds of the executables in this repo can be found at https://kson.dev/games

### Game
Rewrite of unnamed-sdvx-clone.

### Editor
Chart editor for the KSON format with some basic Ksh support.

### KSON
[![Latest version](https://img.shields.io/crates/v/kson.svg)](https://crates.io/crates/kson)
[![Documentation](https://docs.rs/kson/badge.svg)](https://docs.rs/kson)

Library implementing the KSON Chart format.

### kson-music-playback
Library for effected playback of kson charts. Using rodio.

### kson-rodio-sources
Library containing rodio sources implementing the sound effects of kson.
This does not have any dependencies to any of the other projects in this repo
so the effects can be used without bloat in any other project.
