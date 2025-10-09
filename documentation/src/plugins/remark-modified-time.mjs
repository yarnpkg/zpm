import {execSync} from 'child_process';

function getDateFromFilename(filename) {
  // Match the YYYY-MM-DD pattern at the start of the filename
  const dateMatch = filename.match(/^(\d{4}-\d{2}-\d{2})/);
  if (dateMatch)
    return dateMatch[0]; // Returns "2020-01-24"

  return null;
}

export function remarkModifiedTime() {
  return function (tree, file) {
    const filename = file.history[0].split(`/`).pop(); // Get just the filename
    const dateFromFilename = getDateFromFilename(filename);

    if (dateFromFilename) {
      file.data.astro.frontmatter.lastModified = dateFromFilename;
    } else {
      // Fallback to git date if filename doesn't contain a date
      const filepath = file.history[0]; // Use the full file path from the file object
      const result = execSync(`git log -1 --pretty="format:%cI" "${filepath}"`);
      file.data.astro.frontmatter.lastModified = result.toString().trim();
    }
  };
}
