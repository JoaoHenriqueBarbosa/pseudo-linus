// Automatically generated from WebAssemblyMemoryPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex prototypeTableWebAssemblyMemoryIndex[4] = {
    { 1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 0, -1 },
};

static constinit const struct HashTableValue prototypeTableWebAssemblyMemoryValues[2] = {
   { "grow"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyMemoryProtoFuncGrow, 1 } },
   { "buffer"_s, static_cast<unsigned>(PropertyAttribute::ReadOnly|PropertyAttribute::CustomAccessor), NoIntrinsic, { HashTableValue::GetterSetterType, webAssemblyMemoryProtoGetterBuffer, 0 } },
};

static constinit const struct HashTable prototypeTableWebAssemblyMemory =
    { 2, 3, true, nullptr, prototypeTableWebAssemblyMemoryValues, prototypeTableWebAssemblyMemoryIndex };

} // namespace JSC
