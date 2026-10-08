// Automatically generated from WebAssemblyExceptionPrototype.cpp using create_hash_table. DO NOT EDIT!

#include "Lookup.h"

namespace JSC {

static constinit const struct CompactHashIndex prototypeTableWebAssemblyExceptionIndex[8] = {
    { -1, -1 },
    { 2, -1 },
    { 1, -1 },
    { -1, -1 },
    { -1, -1 },
    { -1, -1 },
    { 0, -1 },
    { -1, -1 },
};

static constinit const struct HashTableValue prototypeTableWebAssemblyExceptionValues[3] = {
   { "getArg"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyExceptionProtoFuncGetArg, 2 } },
   { "is"_s, static_cast<unsigned>(PropertyAttribute::Function), NoIntrinsic, { HashTableValue::NativeFunctionType, webAssemblyExceptionProtoFuncIs, 1 } },
   { "stack"_s, static_cast<unsigned>(PropertyAttribute::ReadOnly|PropertyAttribute::CustomAccessor), NoIntrinsic, { HashTableValue::GetterSetterType, webAssemblyExceptionProtoGetterStack, 0 } },
};

static constinit const struct HashTable prototypeTableWebAssemblyException =
    { 3, 7, true, nullptr, prototypeTableWebAssemblyExceptionValues, prototypeTableWebAssemblyExceptionIndex };

} // namespace JSC
