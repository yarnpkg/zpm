import ejs from 'ejs';
import fs from 'fs';
import path from 'path';

const testReport = JSON.parse(fs.readFileSync('artifacts/test-report.json', 'utf8'));

// Process test results to group by full ancestor path
function processTestResults(results) {
    const groups = {};
    
    for (const result of results) {
        const groupKey = result.ancestorTitles.join(' › ');
        
        if (!groups[groupKey]) {
            groups[groupKey] = [];
        }
        
        groups[groupKey].push({
            title: result.title,
            status: result.status,
            duration: result.duration
        });
    }
    
    return groups;
}

// Read the template file
const templatePath = path.join('.github', 'templates', 'downloads.ejs');
const template = fs.readFileSync(templatePath, 'utf8');

// Make processTestResults available to the template
const data = {
    testReport,
    processTestResults
};

const html = ejs.render(template, data);
fs.writeFileSync('artifacts/index.html', html);
console.log('Test report generated: artifacts/index.html');
