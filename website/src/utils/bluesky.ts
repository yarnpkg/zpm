const API_BASE = `https://public.api.bsky.app/xrpc`;

interface BskyFacetFeature {
  $type: string;
  uri?: string;
  did?: string;
  tag?: string;
}

interface BskyFacet {
  index: {byteStart: number, byteEnd: number};
  features: Array<BskyFacetFeature>;
}

interface BskyPostRecord {
  text: string;
  createdAt: string;
  facets?: Array<BskyFacet>;
  reply?: unknown;
}

interface BskyPost {
  uri: string;
  author: {
    handle: string;
    displayName: string;
    avatar: string;
  };
  record: BskyPostRecord;
  likeCount: number;
}

interface BskyFeedItem {
  post: BskyPost;
  reason?: unknown;
}

export interface Skeet {
  avatarUrl: string;
  name: string;
  handle: string;
  sortDate: Date;
  date: string;
  text: string;
  postUrl: string;
  likeCount: number;
}

function renderFacets(text: string, facets?: Array<BskyFacet>): string {
  if (!facets || facets.length === 0)
    return escapeHtml(text);

  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const bytes = encoder.encode(text);

  const sorted = [...facets].sort((a, b) => a.index.byteStart - b.index.byteStart);

  let html = ``;
  let cursor = 0;

  for (const facet of sorted) {
    const {byteStart, byteEnd} = facet.index;
    if (byteStart < cursor) continue;

    html += escapeHtml(decoder.decode(bytes.slice(cursor, byteStart)));

    const segment = escapeHtml(decoder.decode(bytes.slice(byteStart, byteEnd)));

    const link = facet.features.find(f => f.$type === `app.bsky.richtext.facet#link`);
    const mention = facet.features.find(f => f.$type === `app.bsky.richtext.facet#mention`);
    const tag = facet.features.find(f => f.$type === `app.bsky.richtext.facet#tag`);

    if (link?.uri)
      html += `<a href="${escapeAttr(link.uri)}" target="_blank" rel="noopener">${segment}</a>`;
    else if (mention?.did)
      html += `<a href="https://bsky.app/profile/${escapeAttr(mention.did)}" target="_blank" rel="noopener">${segment}</a>`;
    else if (tag?.tag)
      html += `<a href="https://bsky.app/hashtag/${escapeAttr(tag.tag)}" target="_blank" rel="noopener">${segment}</a>`;
    else
      html += segment;


    cursor = byteEnd;
  }

  html += escapeHtml(decoder.decode(bytes.slice(cursor)));
  return html;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`);
}

function escapeAttr(s: string): string {
  return s
    .replace(/&/g, `&amp;`)
    .replace(/"/g, `&quot;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`);
}

function postUrlFromUri(uri: string, handle: string): string {
  const rkey = uri.split(`/`).pop();
  return `https://bsky.app/profile/${handle}/post/${rkey}`;
}

export async function fetchSkeets(handle: string, limit = 5): Promise<Array<Skeet>> {
  try {
    const url = `${API_BASE}/app.bsky.feed.getAuthorFeed?actor=${encodeURIComponent(handle)}&limit=${limit * 2}`;

    const res = await fetch(url);
    if (!res.ok) return [];

    const data = await res.json() as {feed: Array<BskyFeedItem>};

    return data.feed
      .filter(item => !item.reason && !item.post.record.reply)
      .slice(0, limit)
      .map(({post}) => {
        const sortDate = new Date(post.record.createdAt);
        return {
          avatarUrl: post.author.avatar,
          name: post.author.displayName,
          handle: `@${post.author.handle}`,
          sortDate,
          date: sortDate.toLocaleDateString(`en-US`, {month: `short`, day: `numeric`}),
          text: renderFacets(post.record.text, post.record.facets),
          postUrl: postUrlFromUri(post.uri, post.author.handle),
          likeCount: post.likeCount,
        };
      });
  } catch {
    return [];
  }
}
