// Copyright 2017 Google Inc.
//
// Use of this source code is governed by a MIT-style
// license that can be found in the LICENSE file or at
// https://opensource.org/licenses/MIT.

// Porte pseudo-linus: só o find. O xargs do pseudo-linus é reescrito no crate ul-findutils (o do
// uutils diverge do GNU nas opções, nas mensagens e no executor); locate e updatedb ficam de fora.
pub mod find;
