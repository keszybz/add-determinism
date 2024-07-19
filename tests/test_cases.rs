/* SPDX-License-Identifier: GPL-3.0-or-later */

mod test_handlers;

use add_determinism::simplelog;

#[ctor::ctor]
fn init() {
    simplelog::init_with_level(log::LevelFilter::Debug).unwrap();
}
