// Demo for trying the language server by hand; the extension's README says what to try on each line.
local util = import 'lib/util.libsonnet';
local missing = import 'nope.libsonnet';
local { greet, config: { name } } = util;
local add(a, b = 1) = a + b;

{
  hello: greet(name),
  total: add(1, 2),
  nested: { deep: self.again, again: $.total },
  lengths: std.length([1, 2]),
  viaIndex: util['config'].name,
  oops: undefinedVariable,
}
