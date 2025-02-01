import { useState } from 'react';
import report from '../../../artifacts/test-report.json';

interface TestResult {
  title: string;
  status: string;
  duration: number;
  ancestorTitles: string[];
}

interface TestReport {
  numFailedTests: number;
  numPassedTests: number;
  numPendingTests: number;
  numTotalTests: number;
  testResults: Array<{
    assertionResults: TestResult[];
  }>;
}

type FilterState = 'all' | 'failed';

function processTestResults(results: TestResult[]) {
  const groups: Record<string, TestResult[]> = {};
  
  for (const result of results) {
    const groupKey = result.ancestorTitles.join(' › ');
    if (!groups[groupKey]) {
      groups[groupKey] = [];
    }
    groups[groupKey].push(result);
  }
  
  return groups;
}

function Switch({ checked, onChange }: { checked: boolean; onChange: () => void }) {
  return (
    <button
      onClick={onChange}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
        checked ? 'bg-green-500' : 'bg-gray-300'
      }`}
    >
      <span
        className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  );
}

function SearchIcon() {
  return (
    <svg className="w-5 h-5 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg className="w-5 h-5 text-green-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7" />
    </svg>
  );
}

function CrossIcon() {
  return (
    <svg className="w-5 h-5 text-red-500" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
    </svg>
  );
}

function Card({ children, className = '' }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={`bg-white rounded-lg shadow-lg p-6 ${className}`}>
      {children}
    </div>
  );
}

function SearchBar({ 
  value, 
  onChange,
  showSuccessful,
  onToggleSuccessful,
}: { 
  value: string;
  onChange: (value: string) => void;
  showSuccessful: boolean;
  onToggleSuccessful: () => void;
}) {
  return (
    <div className="flex items-center space-x-4">
      <div className="flex-1 flex items-center space-x-4">
        <SearchIcon />
        <input
          type="text"
          placeholder="Search tests..."
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className="w-full px-4 py-2 rounded-lg bg-gray-50 border border-gray-200 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>
      <div className="flex items-center space-x-3 text-sm text-gray-600">
        <span>Show successful tests</span>
        <Switch checked={showSuccessful} onChange={onToggleSuccessful} />
      </div>
    </div>
  );
}

function TestGrid({ results }: { results: TestResult[] }) {
  return (
    <Card className="mb-8">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(12px,1fr))] gap-0.5 max-w-[800px]">
        {results.map((test, index) => {
          const anchor = `${test.ancestorTitles.join('-')}-${test.title}`.toLowerCase().replace(/[^a-z0-9]+/g, '-');
          const statusClass = test.status === 'passed' ? 'bg-green-500' : 'bg-red-500';

          return (
            <a 
              key={index} 
              href={`#${anchor}`} 
              className={`aspect-square rounded hover:ring-2 hover:ring-offset-1 hover:ring-blue-500 transition-all ${statusClass}`} 
              title={`${test.ancestorTitles.join(' › ')} › ${test.title}`}
            />
          );
        })}
      </div>
    </Card>
  )
}

function TestGroups({ results }: { results: TestResult[] }) {
  const groups = processTestResults(results);

  if (Object.keys(groups).length === 0) {
    return (
      <div className="text-center py-12 text-gray-500">
        No tests match your criteria
      </div>
    );
  }

  return (
    <div className="space-y-8">
      {Object.entries(groups).map(([groupPath, tests]) => (
        <div key={groupPath} className="border-t border-gray-200 pt-6 first:border-t-0 first:pt-0">
          <h3 className="text-lg font-semibold mb-4 text-gray-700">
            {groupPath}
          </h3>

          <div className="space-y-2">
            {tests.map((test, index) => {
              const anchor = `${test.ancestorTitles.join('-')}-${test.title}`.toLowerCase().replace(/[^a-z0-9]+/g, '-');
              const statusClass = test.status === 'passed' ? 'bg-green-50' : 'bg-red-50';

              return (
                <a 
                  key={index} 
                  id={anchor} 
                  href={`#${anchor}`} 
                  className={`flex items-center space-x-2 p-2 rounded-lg ${statusClass} hover:ring-2 hover:ring-blue-500 hover:ring-offset-2`}
                >
                  {test.status === 'passed' ? <CheckIcon /> : <CrossIcon />}
                  <span className="flex-1">{test.title}</span>
                  <span className="text-sm text-gray-500">{test.duration}ms</span>
                </a>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  )
}

export default function App() {
  const [showSuccessful, setShowSuccessful] = useState(true);
  const [search, setSearch] = useState('');
  
  const allResults = report.testResults[0].assertionResults;
  const filteredResults = allResults.filter(result => {
    const matchesFilter = showSuccessful || result.status !== 'passed';

    const searchTerm = search.toLowerCase();
    const matchesSearch = 
      search === '' ||
      result.title.toLowerCase().includes(searchTerm) ||
      result.ancestorTitles.some(title => title.toLowerCase().includes(searchTerm));

    return matchesFilter && matchesSearch;
  });

  return (
    <div className="bg-gray-100 min-h-screen p-8">
      <div className="max-w-7xl mx-auto">
        <TestGrid results={allResults} />
        <Card>
          <div className="mb-6">
            <SearchBar 
              value={search} 
              onChange={setSearch} 
              showSuccessful={showSuccessful} 
              onToggleSuccessful={() => setShowSuccessful(!showSuccessful)}
            />
          </div>
          <TestGroups results={filteredResults} />
        </Card>
      </div>
    </div>
  );
} 
