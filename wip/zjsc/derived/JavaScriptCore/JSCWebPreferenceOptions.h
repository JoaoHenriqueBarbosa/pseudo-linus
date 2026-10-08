/*
 * THIS FILE WAS AUTOMATICALLY GENERATED, DO NOT EDIT.
 *
 * Copyright (C) 2026 Apple Inc. All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *
 * THIS SOFTWARE IS PROVIDED BY APPLE INC. AND ITS CONTRIBUTORS ``AS IS''
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
 * THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL APPLE INC. OR ITS CONTRIBUTORS
 * BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF
 * THE POSSIBILITY OF SUCH DAMAGE.
 */

#pragma once

// JSC feature-flag options whose source of truth is
// Source/WTF/Scripts/Preferences/UnifiedWebPreferences.yaml. This macro is
// spliced into FOR_EACH_JSC_OPTION() in runtime/OptionsList.h so these options
// are declared exactly as if written there directly. Feature flags are always
// Normal availability.

#define FOR_EACH_JSC_WEB_PREFERENCE_OPTION(v) \
    v(Bool, useAsyncStackTrace, true, Normal, "Enable async stack traces"_s) \
    v(Bool, useBigIntMathMethods, false, Normal, "Enable BigInt math helper methods."_s) \
    v(Bool, useExplicitResourceManagement, true, Normal, "Enable explicit resource management builtins and syntax."_s) \
    v(Bool, useImportDefer, true, Normal, "Enable deferred module import."_s) \
    v(Bool, useImportText, true, Normal, "Enable text module import."_s) \
    v(Bool, useIteratorChunking, true, Normal, "Expose the Iterator.prototype.chunks and Iterator.prototype.windows methods."_s) \
    v(Bool, useIteratorIncludes, true, Normal, "Expose the Iterator.includes method."_s) \
    v(Bool, useIteratorJoin, true, Normal, "Expose the Iterator.prototype.join method."_s) \
    v(Bool, useIteratorSequencing, true, Normal, "Expose the Iterator.concat method."_s) \
    v(Bool, useJSONSourceTextAccess, true, Normal, "Expose JSON source text access feature."_s) \
    v(Bool, useJSPI, true, Normal, "Enable the implementation of JavaScript Promise Integration."_s) \
    v(Bool, useJointIteration, true, Normal, "Expose the Iterator.zip and Iterator.zipKeyed methods"_s) \
    v(Bool, useMoreCurrencyDisplayChoices, false, Normal, "Enable more currencyDisplay choices for Intl.NumberFormat"_s) \
    v(Bool, usePromiseIsPromise, false, Normal, "Expose the Promise.isPromise method."_s) \
    v(Bool, useRegExpBufferBoundaries, false, Normal, "Allow the use in regular expressions of \\A, \\z and \\Z from the RegExp Buffer Boundaries proposal"_s) \
    v(Bool, useShadowRealm, false, Normal, "Expose the ShadowRealm object."_s) \
    v(Bool, useTemporal, true, Normal, "Expose the Temporal object."_s) \
    v(Bool, useWasmJSStringBuiltins, true, Normal, "Enable the implementation of the JS String Builtins proposal."_s) \
    v(Bool, useWasmJSTypes, false, Normal, "Enable the js-types proposal: type() on WebAssembly.Memory/Table/Global/Tag, and the type field on WebAssembly.Module.imports/exports descriptors."_s) \
    v(Bool, useWasmMemory64, true, Normal, "Allow the Memory64 proposal for WebAssembly."_s) \
    v(Bool, useWasmMemoryToBufferAPIs, true, Normal, "Enable the toFixedLengthBuffer() and toResizableBuffer() Wasm Memory.prototype functions."_s) \
    v(Bool, useWasmMultiMemory, true, Normal, "Allow wasm code to access multiple linear memories"_s) \
    v(Bool, useWasmRelaxedSIMD, true, Normal, "Allow the relaxed simd instructions and types from the wasm relaxed simd spec."_s) \
    v(Bool, useWasmSIMD, true, Normal, "Allow the new simd instructions and types from the wasm simd spec."_s) \
    v(Bool, useWasmTailCalls, true, Normal, "Allow the new instructions from the wasm tail calls spec."_s) \
    v(Bool, useWasmWideArithmetic, false, Normal, "Allow the wide arithmetic instructions from the wasm wide-arithmetic spec."_s) \


// The subset of the above whose feature status is "experimental" (developer/testable/preview/
// stable)
#define FOR_EACH_JSC_EXPERIMENTAL_WEB_PREFERENCE_OPTION(v) \
    v(useAsyncStackTrace) \
    v(useBigIntMathMethods) \
    v(useExplicitResourceManagement) \
    v(useImportDefer) \
    v(useImportText) \
    v(useIteratorChunking) \
    v(useIteratorIncludes) \
    v(useIteratorJoin) \
    v(useIteratorSequencing) \
    v(useJSONSourceTextAccess) \
    v(useJSPI) \
    v(useJointIteration) \
    v(useMoreCurrencyDisplayChoices) \
    v(usePromiseIsPromise) \
    v(useRegExpBufferBoundaries) \
    v(useShadowRealm) \
    v(useTemporal) \
    v(useWasmJSStringBuiltins) \
    v(useWasmJSTypes) \
    v(useWasmMemory64) \
    v(useWasmMemoryToBufferAPIs) \
    v(useWasmMultiMemory) \
    v(useWasmRelaxedSIMD) \
    v(useWasmSIMD) \
    v(useWasmTailCalls) \
    v(useWasmWideArithmetic) \

