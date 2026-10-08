// Automatically generated from StringConstructor.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex stringConstructorTableIndex[9] = {
    { -1, -1 },
    { 0, 8 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 2, -1 },
    { 1, -1 },
};

static constinit const struct HashTableValue stringConstructorTableValues[3] = {
   { "fromCharCode"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), FromCharCodeIntrinsic, { HashTableValue::NativeFunctionType, stringFromCharCode, 1 } },
   { "fromCodePoint"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), FromCodePointIntrinsic, { HashTableValue::NativeFunctionType, stringFromCodePoint, 1 } },
   { "raw"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, stringRaw, 1 } },
};

static constinit const struct HashTable stringConstructorTable =
    { 3, 7, false, nullptr, stringConstructorTableValues, stringConstructorTableIndex };

} // namespace JSC
