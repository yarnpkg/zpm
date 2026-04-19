import {GithubMarkdown}                           from '../components/github-markdown';
import {useAppSelector}                           from '../store/hooks';
import {selectIsConnected}                        from '../store/slices/connectionSlice';
import {selectReadmeContent, selectReadmeLoading} from '../store/slices/readmeSlice';

export function DashboardRoute() {
  const isConnected = useAppSelector(selectIsConnected);
  const content = useAppSelector(selectReadmeContent);
  const loading = useAppSelector(selectReadmeLoading);

  return (
    <div className={`p-8`}>
      {!isConnected ? (
        <p className={`rounded border border-yellow-300 bg-yellow-50 p-3 text-yellow-800`}>
          Waiting for daemon connection…
        </p>
      ) : loading ? (
        <p className={`text-slate-500`}>Loading…</p>
      ) : content !== null ? (
        <GithubMarkdown content={content} className={`max-w-screen-md bg-white shadow-sm p-8`} />
      ) : (
        <p className={`text-slate-500`}>No README.md found in the project root.</p>
      )}
    </div>
  );
}
