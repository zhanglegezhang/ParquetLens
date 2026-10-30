import test from 'node:test';
import assert from 'node:assert/strict';
import {compareValues,tsv} from '../src/lib/columns.ts';
test('numeric sorting preserves 64-bit and Decimal differences',()=>{
  assert(compareValues('9007199254740993','9007199254740992')>0);
  assert(compareValues('-0.0000000000000002','-0.0000000000000001')<0);
  assert(compareValues('10','2')>0);assert.equal(compareValues('001.20','1.2'),0);
  assert.equal(compareValues('-0','0'),0);assert(compareValues(null,'1')>0);
});
test('TSV protects separators and quotes without altering data',()=>{
  assert.equal(tsv([['列','值'],['中文\t🦀','a"b\nc'],['x','=cmd']]),'列\t值\r\n"中文\t🦀"\t"a""b\nc"\r\nx\t=cmd');
});
