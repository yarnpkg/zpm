#!/usr/bin/env node

import cp from 'child_process';
import fs from 'fs/promises';
import path from 'path';
import { Command, Option, runExit } from 'clipanion';

runExit(class ImportArtifactsCommand extends Command {
  static usage = Command.Usage({
    description: 'Generate the ZPM artifacts based on the content from a local Berry repository',
  });

  berryFolder = Option.String('--berry-folder', {
    required: true,
    description: 'Path to the Berry repository',
  });

  async execute() {
    await this.importPatches();
    await this.importPackageExtensions();
  }

  async importPatches() {
    const patchesDir = path.join(this.berryFolder, 'packages/plugin-compat/sources/patches');
    const outputDir = path.join(process.cwd(), 'packages/zpm/patches');

    // Ensure output directory exists
    await fs.mkdir(outputDir, { recursive: true });

    // Process all patches
    const files = await fs.readdir(patchesDir);
    const patchFiles = files.filter(f => f.endsWith('.patch.ts'));
    
    for (const file of patchFiles) {
      const name = file.replace('.patch.ts', '');
      await this.processPatch(patchesDir, outputDir, name);
    }
    
    console.log(`Processed ${patchFiles.length} patches`);
  }

  async processPatch(patchesDir, outputDir, name) {
    const sourcePath = path.join(patchesDir, `${name}.patch.ts`);
    const source = await fs.readFile(sourcePath, 'utf8');
    
    const match = source.match(/brotliDecompressSync\((Buffer\.from\(.*?, `base64`\))\)/);
    if (!match)
      throw new Error(`Could not find brotli-compressed content in ${name}.patch.ts`);
      
    const payload = match[1];
    const buffer = eval(`(${payload})`);
    
    const outputPath = path.join(outputDir, `${name}.brotli.dat`);
    await fs.writeFile(outputPath, buffer);
    
    console.log(`  ✓ ${name}`);
  }

  async importPackageExtensions() {
    const outputPath = path.join(process.cwd(), 'packages/zpm/data/builtin-extensions.json');

    const dump = cp.execFileSync(`yarn`, [`dump:extensions`], {
      cwd: this.berryFolder,
      encoding: 'utf-8',
    });

    const entries = JSON.parse(dump);
    const serialized = JSON.stringify(entries);

    await fs.writeFile(outputPath, serialized);

    console.log(`Imported ${Object.keys(entries).length} package extensions`);
  }
});
