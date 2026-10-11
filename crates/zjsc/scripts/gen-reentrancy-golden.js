// Gera tests/golden/reentrancy_bun.tsv: cada linha é `id<TAB>programa<TAB>resultado`, com o resultado
// medido no bun (o JavaScriptCore real). Rodar: bun scripts/gen-reentrancy-golden.js
// Cada programa é de uma linha, sem tab, e o harness (tests/golden/reentrancy_bun_harness.js) o avalia por
// eval indireto e serializa o valor. Todos os laços são limitados, para nunca travar.
const fs = require('fs');
const path = require('path');

const programs = [
  // sort com comparador que muta o array
  ['sort_length_zero', "var a=[5,3,1,4,2];a.sort(function(x,y){a.length=0;return x-y});a"],
  ['sort_push', "var a=[5,3,1,4,2],n=0;a.sort(function(x,y){if(n++<3)a.push(9);return x-y});a"],
  ['sort_pop', "var a=[5,3,1,4,2];a.sort(function(x,y){a.pop();return x-y});a"],
  ['sort_splice', "var a=[5,3,1,4,2],n=0;a.sort(function(x,y){if(n++==1)a.splice(0,3);return x-y});a"],
  ['sort_delete', "var a=[5,3,1,4,2],n=0;a.sort(function(x,y){if(n++==1)delete a[0];return x-y});a"],
  ['sort_setproto_holes', "var a=[3,,1,2];a.sort(function(x,y){Object.setPrototypeOf(a,{1:7});return x-y});[a,Object.keys(a)]"],
  ['sort_freeze', "(function(){var a=[3,2,1],r;try{a.sort(function(x,y){Object.freeze(a);return x-y})}catch(e){r=e.name}return [r,a]})()"],
  ['tosorted_mutates_source', "var a=[3,2,1,5,4];var r=a.toSorted(function(x,y){a.length=1;return x-y});[r,a]"],
  ['sort_comparator_throws', "(function(){var a=[4,3,2,1],n=0,r;try{a.sort(function(x,y){if(++n==3)throw new Error('x');return x-y})}catch(e){r=e.message}return [r,a.length]})()"],
  ['sort_reentrant_sort', "var a=[3,1,2],n=0;a.sort(function(x,y){if(n++==0)a.sort();return x-y});a"],
  ['sort_sparse_define', "var a=[3,,,1];a.sort(function(x,y){a[1]=0;return x-y});[a,a.length]"],
  ['sort_arraylike_length', "var o={0:3,1:1,2:2,length:3};Array.prototype.sort.call(o,function(x,y){o.length=1;return x-y});[o[0],o[1],o[2],o.length]"],
  ['sort_shift_unshift', "var a=[3,1,2],n=0;a.sort(function(x,y){if(n++<2){a.shift();a.unshift(8,9)}return x-y});a"],
  ['sort_comparator_returns_object_valueof', "var a=[3,1,2];a.sort(function(x,y){return {valueOf:function(){a.length=0;return x-y}}});a"],

  // callbacks de iteração que mutam o array
  ['map_length_zero', "var a=[1,2,3,4];a.map(function(x,i){a.length=0;return x*2})"],
  ['map_push', "var a=[1,2,3],n=0;var r=a.map(function(x){if(n++<5)a.push(x);return x});[r,a.length]"],
  ['filter_splice', "var a=[1,2,3,4,5];a.filter(function(x,i){a.splice(0,1);return true})"],
  ['filter_pop', "var a=[1,2,3,4,5];[a.filter(function(x){a.pop();return true}),a]"],
  ['foreach_delete_next', "var a=[1,2,3,4],r=[];a.forEach(function(x,i){r.push(x);delete a[i+1]});r"],
  ['foreach_push_not_visited', "var a=[1,2,3],r=[];a.forEach(function(x){r.push(x);a.push(x+10)});[r,a.length]"],
  ['reduce_length_zero', "var a=[1,2,3,4];a.reduce(function(s,x){a.length=0;return s+x})"],
  ['reduce_pop', "var a=[1,2,3,4,5];a.reduce(function(s,x){a.pop();return s+x},0)"],
  ['reduceright_splice', "var a=[1,2,3,4,5];a.reduceRight(function(s,x){a.splice(0,2);return s+','+x},'')"],
  ['find_shrinks', "var a=[1,2,3,4];a.find(function(x,i){a.length=2;return x==4})"],
  ['findlast_shrinks', "var a=[1,2,3,4];a.findLast(function(x,i){a.length=1;return x==2})"],
  ['findindex_delete', "var a=[1,2,3];a.findIndex(function(x,i){delete a[2];return x===undefined})"],
  ['some_pop', "var a=[1,2,3,4];[a.some(function(x){a.pop();return x==4}),a]"],
  ['every_length_zero', "var a=[1,2,3];[a.every(function(x){a.length=0;return true}),a.length]"],
  ['flat_getter_mutates', "var a=[[1],[2],[3]];Object.defineProperty(a,1,{get:function(){a.length=0;return [9]},configurable:true});a.flat()"],
  ['flatmap_mutates', "var a=[1,2,3,4];a.flatMap(function(x){a.length=2;return [x,x]})"],
  ['array_from_arraylike_getter', "var o={length:3,get 0(){o.length=1;return 'a'},1:'b',2:'c'};Array.from(o)"],
  ['array_from_mapfn_mutates', "var o={length:4,0:1,1:2,2:3,3:4};Array.from(o,function(x){o.length=2;return x})"],
  ['toSpliced_with_proxy_len', "var a=[1,2,3,4];var p=new Proxy(a,{get:function(t,k,r){if(k==='length'){return 2}return Reflect.get(t,k,r)}});Array.prototype.toSpliced.call(p,0,1)"],
  ['copywithin_valueof', "var a=[1,2,3,4,5];var s={valueOf:function(){a.length=2;return 0}};a.copyWithin(s,3);[a,a.length]"],
  ['copywithin_end_valueof', "var a=[1,2,3,4,5];a.copyWithin(0,{valueOf:function(){a.length=0;return 2}},4);[a,a.length]"],
  ['fill_valueof_start', "var a=[1,2,3,4,5];a.fill(0,{valueOf:function(){a.length=2;return 0}},5);[a,a.length]"],
  ['fill_valueof_end', "var a=[1,2,3];a.fill(7,0,{valueOf:function(){a.length=1;return 3}});[a,a.length]"],
  ['includes_fromindex_valueof', "var a=[1,2,3,4];[a.includes(4,{valueOf:function(){a.length=1;return 0}}),a.length]"],
  ['indexof_fromindex_valueof', "var a=[1,2,3,4];[a.indexOf(undefined,{valueOf:function(){a.length=6;return 0}}),a.length]"],
  ['lastindexof_fromindex_valueof', "var a=[1,2,3,4];[a.lastIndexOf(4,{valueOf:function(){a.length=1;return 3}}),a.length]"],
  ['slice_valueof_end', "var a=[1,2,3,4,5];a.slice(0,{valueOf:function(){a.length=2;return 5}})"],
  ['splice_valueof_count', "var a=[1,2,3,4,5];var r=a.splice(1,{valueOf:function(){a.length=2;return 3}});[r,a]"],
  ['at_valueof', "var a=[1,2,3];a.at({valueOf:function(){a.length=0;return 1}})"],
  ['join_tostring_mutates', "var a=[1,{toString:function(){a.length=1;return 'x'}},3];a.join('-')"],
  ['join_tostring_grow', "var a=[1,{toString:function(){a.push(4,5);return 'x'}},3];a.join('-')"],
  ['concat_spreadable_getter', "var a=[1,2];var o={length:3,0:'a',1:'b',2:'c'};Object.defineProperty(o,Symbol.isConcatSpreadable,{get:function(){a.length=0;return true}});a.concat(o)"],
  ['reverse_getter_mutates', "var a=[1,2,3,4];Object.defineProperty(a,1,{get:function(){a.length=2;return 'g'},configurable:true});a.reverse();[a.length,Object.keys(a)]"],
  ['indexof_proto_getter', "var a=[1,,3];Object.defineProperty(Array.prototype,'1',{get:function(){a.length=0;return 9},configurable:true});var r=a.indexOf(9);delete Array.prototype[1];r"],

  // Map / Set
  ['map_foreach_delete_self', "var m=new Map([[1,'a'],[2,'b'],[3,'c']]),r=[];m.forEach(function(v,k){r.push(k);m.delete(k+1)});r"],
  ['map_foreach_clear', "var m=new Map([[1,'a'],[2,'b'],[3,'c']]),r=[];m.forEach(function(v,k){r.push(k);m.clear()});[r,m.size]"],
  ['map_foreach_add', "var m=new Map([[1,'a'],[2,'b']]),r=[];m.forEach(function(v,k){r.push(k);if(k<6)m.set(k+2,'n')});r"],
  ['map_foreach_delete_readd', "var m=new Map([[1,'a'],[2,'b']]),r=[],n=0;m.forEach(function(v,k){r.push(k);if(n++<3){m.delete(k);m.set(k,v)}});r"],
  ['set_foreach_delete', "var s=new Set([1,2,3,4]),r=[];s.forEach(function(v){r.push(v);s.delete(v+1)});r"],
  ['set_foreach_clear_add', "var s=new Set([1,2,3]),r=[];s.forEach(function(v){r.push(v);if(r.length==1){s.clear();s.add(9)}});r"],
  ['map_iterator_delete_during', "var m=new Map([[1,1],[2,2],[3,3]]),r=[];for(var e of m){r.push(e[0]);m.delete(2);if(e[0]==1)m.set(4,4)}r"],
  ['set_iterator_clear_then_add', "var s=new Set([1,2,3]),r=[];for(var v of s){r.push(v);if(v==1){s.clear();s.add(7)}}r"],
  ['set_union_setlike_mutates', "var s=new Set([1,2,3]);var o={size:2,has:function(){s.clear();return true},keys:function(){return [5,6][Symbol.iterator]()}};[Array.from(s.union(o)),s.size]"],
  ['set_intersection_has_mutates', "var s=new Set([1,2,3,4]);var o={size:10,has:function(v){s.delete(v+1);return true},keys:function(){return [][Symbol.iterator]()}};Array.from(s.intersection(o))"],
  ['set_difference_has_mutates', "var s=new Set([1,2,3,4]);var o={size:1,has:function(v){s.clear();return false},keys:function(){return [][Symbol.iterator]()}};Array.from(s.difference(o))"],
  ['weakmap_getter_reentry', "var w=new WeakMap(),k={};w.set(k,1);var o=Object.defineProperty({},'x',{get:function(){w.delete(k);return 2}});w.set(k,o.x);w.get(k)"],

  // JSON, Object.*
  ['json_stringify_tojson_mutates', "var o={a:{toJSON:function(){delete o.b;o.c=3;return 1}},b:2};JSON.stringify(o)"],
  ['json_stringify_array_tojson_length', "var a=[{toJSON:function(){a.length=1;return 'x'}},2,3];JSON.stringify(a)"],
  ['json_stringify_replacer_mutates', "var o={a:1,b:2,c:3};JSON.stringify(o,function(k,v){if(k==='a')delete o.c;return v})"],
  ['json_stringify_replacer_array_proxy', "var arr=['a','b'];JSON.stringify({a:1,b:2,c:3},new Proxy(arr,{get:function(t,k,r){if(k==='length')arr.length=1;return Reflect.get(t,k,r)}}))"],
  ['json_parse_reviver_mutates', "var r=[];JSON.parse('{\"a\":1,\"b\":[1,2,3],\"c\":3}',function(k,v){r.push(k);if(k==='a')delete this.c;if(k==='0')this.length=1;return v});r"],
  ['object_assign_getter_deletes', "var s={get a(){delete s.b;return 1},b:2,c:3};Object.assign({},s)"],
  ['object_keys_proxy_ownkeys_mutates', "var t={a:1,b:2};var p=new Proxy(t,{ownKeys:function(){delete t.a;return ['a','b']},getOwnPropertyDescriptor:function(tt,k){return Reflect.getOwnPropertyDescriptor(tt,k)}});Object.keys(p)"],
  ['object_entries_getter_deletes', "var o={get a(){delete o.b;return 1},b:2,c:3};Object.entries(o)"],
  ['object_values_getter_clears', "var o={get a(){for(var k in o)if(k!=='a')delete o[k];return 1},b:2,c:3};Object.values(o)"],
  ['object_fromentries_iterator_mutates', "var a=[['x',1],['y',2],['z',3]];Object.fromEntries({[Symbol.iterator]:function(){var i=0;return {next:function(){if(i==1)a.length=0;return i<a.length?{value:a[i++],done:false}:{done:true}}}}})"],
  ['object_defineproperties_getter', "var d={get a(){delete d.b;return {value:1}},b:{value:2}};var o={};Object.defineProperties(o,d);Object.getOwnPropertyNames(o)"],
  ['forin_delete_during', "var o={a:1,b:2,c:3,d:4},r=[];for(var k in o){r.push(k);delete o['b'];delete o['c']}r"],
  ['forin_add_during', "var o={a:1,b:2},r=[];for(var k in o){r.push(k);o[k+'x']=1}r"],

  // spread / for-of / iteradores
  ['spread_iterator_mutates', "var a=[1,2,3,4];[...{[Symbol.iterator]:function(){var i=0;return {next:function(){if(i==1)a.length=0;return i<a.length?{value:a[i++],done:false}:{done:true}}}}}]"],
  ['forof_array_shrinks', "var a=[1,2,3,4],r=[];for(var v of a){r.push(v);a.length=2}r"],
  ['forof_array_grows', "var a=[1,2],r=[];for(var v of a){r.push(v);if(a.length<5)a.push(v*10)}r"],
  ['forof_array_proto_iterator_patched', "var a=[1,2,3],r=[];var orig=Array.prototype[Symbol.iterator];for(var v of a){r.push(v);Array.prototype[Symbol.iterator]=function(){return [][Symbol.iterator]()}}Array.prototype[Symbol.iterator]=orig;r"],
  ['array_iterator_next_patched', "var a=[1,2,3],r=[];var it=a[Symbol.iterator](),P=Object.getPrototypeOf(it);var n=P.next;for(var v of a){r.push(v);P.next=function(){return {done:true}}}P.next=n;r"],
  ['spread_call_args_mutate', "var a=[1,2,3];function f(){return arguments.length}f(...a,...{[Symbol.iterator]:function(){a.length=0;return [9][Symbol.iterator]()}})"],
  ['destructure_iterator_mutates', "var a=[1,2,3];var [x,y,z]={[Symbol.iterator]:function(){return {next:function(){a.length=0;return {value:a.length,done:false}}}}};[x,y,z]"],
  ['array_from_iterable_mutates', "var src=[1,2,3,4];Array.from({[Symbol.iterator]:function(){var i=0;return {next:function(){if(i==2)src.length=0;return i<4?{value:i++,done:false}:{done:true}}}}},function(v){src.push(v);return v})"],
  ['array_from_set_mutates_in_mapfn', "var s=new Set([1,2,3]);Array.from(s,function(v){s.delete(v+1);return v})"],
  ['promise_all_iterable_mutates', "var a=[1,2,3];var r;Promise.all({[Symbol.iterator]:function(){var i=0;return {next:function(){a.length=1;return i<3?{value:i++,done:false}:{done:true}}}}}).then(function(v){r=v});r"],

  // TypedArray / ArrayBuffer
  ['ta_sort_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);t.set([5,4,3,2,1,0,9,8]);t.sort(function(x,y){b.resize(2);return x-y});[b.byteLength,Array.from(t)]"],
  ['ta_sort_transfer', "var b=new ArrayBuffer(8);var t=new Uint8Array(b);t.set([5,4,3,2,1,0,9,8]);var r;try{t.sort(function(x,y){b.transfer();return x-y})}catch(e){r=e.name}[r,t.length,b.detached]"],
  ['ta_map_resize', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);t.set([1,2,3,4]);var r=t.map(function(x,i){b.resize(2);return x*2});[Array.from(r),t.length]"],
  ['ta_foreach_transfer', "var b=new ArrayBuffer(4);var t=new Uint8Array(b),r=[];t.forEach(function(x,i){r.push(x);if(i==1)b.transfer()});[r,t.length]"],
  ['ta_fill_valueof_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);t.fill({valueOf:function(){b.resize(2);return 7}});[b.byteLength,Array.from(t)]"],
  ['ta_includes_fromindex_shrink', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);[t.includes(0,{valueOf:function(){b.resize(1);return 2}}),t.length]"],
  ['ta_indexof_fromindex_transfer', "var b=new ArrayBuffer(4);var t=new Uint8Array(b);var r;try{r=t.indexOf(0,{valueOf:function(){b.transfer();return 0}})}catch(e){r=e.name}r"],
  ['ta_set_array_getter_resize', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);var src={length:3,get 0(){b.resize(1);return 5},1:6,2:7};var r;try{t.set(src)}catch(e){r=e.name}[r,Array.from(t)]"],
  ['ta_slice_valueof_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);t.set([1,2,3,4,5,6,7,8]);Array.from(t.slice(0,{valueOf:function(){b.resize(3);return 8}}))"],
  ['ta_subarray_tracking_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);var s=t.subarray(2);b.resize(4);var r=[s.length];b.resize(1);r.push(s.length);r"],
  ['ta_copywithin_valueof_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var t=new Uint8Array(b);t.set([1,2,3,4,5,6,7,8]);t.copyWithin(0,{valueOf:function(){b.resize(2);return 4}});[b.byteLength,Array.from(t)]"],
  ['ta_from_mapfn_resize', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);Uint8Array.from(t,function(x,i){b.resize(1);return i})"],
  ['ta_tosorted_comparator_resize', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);t.set([4,3,2,1]);var r=t.toSorted(function(x,y){b.resize(0);return x-y});[Array.from(r),t.length]"],
  ['ta_join_tostring_resize', "var b=new ArrayBuffer(4,{maxByteLength:8});var t=new Uint8Array(b);t.join({toString:function(){b.resize(1);return '-'}})"],
  ['dataview_getter_resize', "var b=new ArrayBuffer(8,{maxByteLength:16});var d=new DataView(b);var r;try{r=d.getUint8({valueOf:function(){b.resize(2);return 5}})}catch(e){r=e.name}r"],
  ['ta_every_detach_midway', "var b=new ArrayBuffer(4);var t=new Uint8Array(b);var n=0;[t.every(function(x){if(n++==1)b.transfer();return true}),n]"],

  // String / RegExp
  ['replace_fn_mutates_lastindex', "var re=/a/g;var r='aaaa'.replace(re,function(m){re.lastIndex=0;return 'b'});r"],
  ['replace_fn_changes_regexp_source', "var re=/a/g;'aaaa'.replace(re,function(m){re.compile('b','g');return 'x'})"],
  ['replace_fn_sets_proto_exec', "var re=/a/g,n=0;var r='aaa'.replace(re,function(m){re.exec=function(){return null};return 'z'});r"],
  ['replaceall_fn_lastindex', "var re=/a/g;'abab'.replaceAll(re,function(m){re.lastIndex=3;return 'q'})"],
  ['match_all_lastindex_mutated', "var re=/a/g,r=[];for(var m of 'aaa'.matchAll(re)){r.push(m.index);re.lastIndex=0;if(r.length>5)break}r"],
  ['split_species_regexp', "var re=/,/;re.constructor={[Symbol.species]:function(){re.lastIndex=5;return /,/y}};'a,b,c'.split(re)"],
  ['regexp_exec_getter_lastindex', "var re=/a/g;re.lastIndex={valueOf:function(){re.lastIndex=0;return 2}};[re.exec('aaaa').index,re.lastIndex]"],
  ['regexp_symbol_replace_replacement_tostring', "var re=/a/g;'aaa'.replace(re,{toString:function(){re.lastIndex=1;return 'x'}})"],
  ['string_replace_searchvalue_tostring', "var s='abcabc';s.replace({toString:function(){return 'b'}},function(){return 'X'})"],
  ['string_padstart_tostring_reenter', "'x'.padStart(5,{toString:function(){return 'ab'}})"],
  ['string_localecompare_arr_sort', "var a=['b','a','c'];a.sort(function(x,y){a.length=1;return x.localeCompare(y)});a"],

  // Proxy
  ['proxy_get_trap_reenters_array', "var a=[1,2,3];var p=new Proxy(a,{get:function(t,k,r){if(k==='length')a.push(0);return Reflect.get(t,k,r)}});Array.prototype.map.call(p,function(x){return x}).length"],
  ['proxy_has_trap_deletes', "var a=[1,2,3];var p=new Proxy(a,{has:function(t,k){delete a[2];return Reflect.has(t,k)}});Array.prototype.forEach.call(p,function(){});a.length"],
  ['proxy_ownkeys_trap_adds', "var t={a:1};var p=new Proxy(t,{ownKeys:function(){t.b=2;return Reflect.ownKeys(t)}});JSON.stringify(p)"],
  ['proxy_set_trap_sorts', "var a=[3,2,1];var n=0;var p=new Proxy(a,{set:function(t,k,v,r){if(n++==1)t.length=0;return Reflect.set(t,k,v,r)}});Array.prototype.sort.call(p);[a,a.length]"],
  ['proxy_deleteproperty_trap_reenters', "var o={a:1,b:2};var p=new Proxy(o,{deleteProperty:function(t,k){delete t.b;return Reflect.deleteProperty(t,k)}});delete p.a;Object.keys(o)"],
  ['proxy_revoked_in_callback', "var r=Proxy.revocable([1,2,3],{});var res;try{res=Array.prototype.map.call(r.proxy,function(x){r.revoke();return x})}catch(e){res=e.name}res"],
  ['proxy_getprototypeof_trap_mutates', "var a=[1,,3];var p=new Proxy({},{get:function(){a.length=0;return 7}});Object.setPrototypeOf(a,p);var r=a.map(function(x){return x});[r.length,Object.keys(r)]"],
  ['reflect_apply_comparator_arraylength', "var a=[3,2,1];a.sort(function(x,y){return Reflect.apply(function(){a.length=2;return x-y},null,[])});a"],
  ['array_species_constructor_mutates', "var a=[1,2,3];a.constructor={[Symbol.species]:function(n){a.length=0;return []}};var r=a.map(function(x){return x*2});[r,a.length]"],
  ['array_species_filter_mutates', "var a=[1,2,3];a.constructor={[Symbol.species]:function(n){a.length=1;return {}}};var r=a.filter(function(x){return true});[Object.keys(r),a.length]"],
  ['array_length_setter_reenter', "var a=[1,2,3];Object.defineProperty(a,'length',{writable:false});var r=[];try{a.push(1)}catch(e){r.push(e.name)}try{a.pop()}catch(e){r.push(e.name)}try{a.splice(0,1)}catch(e){r.push(e.name)}r.concat([a.length])"],
];

if (programs.length < 80) throw new Error('esperado 80 programas, tem ' + programs.length);
const harness = eval(fs.readFileSync(path.join(__dirname, '../tests/golden/reentrancy_bun_harness.js'), 'utf8'));
if (process.argv[2] === '--one') {
  const entry = programs.find((p) => p[0] === process.argv[3]);
  console.log(harness(entry[1]));
  process.exit(0);
}
const seen = new Set();
const lines = [];
for (const [id, source] of programs) {
  if (seen.has(id)) throw new Error('id repetido ' + id);
  seen.add(id);
  if (/[\t\n]/.test(source)) throw new Error('programa com tab ou quebra: ' + id);
  // Cada programa roda isolado em eval indireto; o global sujo de um não pode afetar o seguinte
  // quando o protótipo é alterado, então o gerador roda tudo num processo filho por programa.
  const { spawnSync } = require('child_process');
  const child = spawnSync(process.execPath, [__filename, '--one', id], { encoding: 'utf8', timeout: 20000 });
  const result = child.status === 0 ? child.stdout.replace(/\n$/, '') : 'GENERATOR_FAILED ' + child.stderr;
  lines.push([id, source, result].join('\t'));
}
fs.writeFileSync(path.join(__dirname, '../tests/golden/reentrancy_bun.tsv'), require("./golden-prelude.js").assertPublicResult(lines.join('\n') + '\n'));
console.log('programas:', lines.length);
