// Automatically generated from AsyncGeneratorPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex asyncGeneratorPrototypeTableIndex[4] = {
    { 1, -1 },
    { 0, -1 },
    { -1, -1 },
    { -1, -1 },
};

static constinit const struct HashTableValue asyncGeneratorPrototypeTableValues[2] = {
   { "return"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, asyncGeneratorPrototypeReturn, 1 } },
   { "throw"_s, static_cast<unsigned>(PropertyAttribute::DontEnum|PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, asyncGeneratorPrototypeThrow, 1 } },
};

static constinit const struct HashTable asyncGeneratorPrototypeTable =
    { 2, 3, false, nullptr, asyncGeneratorPrototypeTableValues, asyncGeneratorPrototypeTableIndex };

} // namespace JSC
