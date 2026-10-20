// A tour of the syntax, for judging the highlighting by eye. Not meant to be evaluated.
# Hash comments work too.
/* A block
   comment. */

local greeting = 'single', other = "double \"escaped\" A \n";
local verbatim = @'it''s verbatim, no \n escapes';
local block = |||
  A text block keeps its indentation,
    and "quotes" and // slashes are just text.
|||;
local obj = {
  plain: 1,
  hidden:: 2,
  visible::: 3,
  inherited+: { extra: true },
  method(x, y = 2):: x + y,
  [greeting + '_dynamic']: null,
  local helper = self.plain,
  assert self.plain > 0 : 'plain must be positive',
};
local pick(a, b = 10) = if a > b then a else b;
local numbers = [0, 1.5, 2e3, 1E-2];
local sliced = [1, 2, 3, 4][1:3];
local logic = 1 < 2 && 3 >= 2 || !false;
local bits = 1 << 2 | 3 & 4 ^ 5;
// jrsonnet extensions: destructuring, `??` and `?.`
local { a, b: [c, ...rest] } = { a: 1, b: [2, 3, 4] };
local fallback = null ?? 'default';
local maybe = obj?.plain;

{
  comprehension: [x * 2 for x in numbers if x != 2],
  objectComprehension: { [k]: k + '!' for k in ['a', 'b'] },
  extended: { base: 1 } + { derived: super.base, root: $.base },
  imported: import 'lib/util.libsonnet',
  importedText: importstr 'lib/util.libsonnet',
  call: pick(1) + obj.method(1) + std.length(greeting),
  literals: [true, false, null, self.call],
  unused: [other, verbatim, block, sliced, logic, bits, a, c, rest, fallback, maybe],
}
