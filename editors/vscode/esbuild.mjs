// Bundles the extension, including vscode-languageclient, into one file so the VSIX needs no
// node_modules. The integration suite is bundled next to it for `npm run test:integration`; it
// is not part of the package.
import * as esbuild from 'esbuild';

const watch = process.argv.includes('--watch');
const common = {
  bundle: true,
  external: ['vscode'],
  format: 'cjs',
  platform: 'node',
  target: 'node20',
  sourcemap: true,
  logLevel: 'info',
};
const builds = [
  { ...common, entryPoints: ['src/extension.ts'], outfile: 'out/extension.js' },
  { ...common, entryPoints: ['test-integration/suite.ts'], outfile: 'out/integration.js' },
];

for (const options of builds) {
  if (watch) {
    await (await esbuild.context(options)).watch();
  } else {
    await esbuild.build(options);
  }
}
