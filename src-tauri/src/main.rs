// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ariekei_lib::run()
}
