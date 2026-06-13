import {visit} from 'unist-util-visit';

function escapeHtml(str) {
  return str
    .replace(/&/g, `&amp;`)
    .replace(/</g, `&lt;`)
    .replace(/>/g, `&gt;`)
    .replace(/"/g, `&quot;`);
}

function renderEmbed({handle, displayName, avatar, text, date, likes, postUrl, profileUrl}) {
  const butterfly = `<svg class="bsky-logo-icon" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>Bluesky</title><path d="M5.202 2.857C7.954 4.922 10.913 9.11 12 11.358c1.087-2.247 4.046-6.436 6.798-8.501C20.783 1.366 24 .213 24 3.883c0 .732-.42 6.156-.667 7.037-.856 3.061-3.978 3.842-6.755 3.37 4.854.826 6.089 3.562 3.422 6.299-5.065 5.196-7.28-1.304-7.847-2.97-.104-.305-.152-.448-.153-.327 0-.121-.05.022-.153.327-.568 1.666-2.782 8.166-7.847 2.97-2.667-2.737-1.432-5.473 3.422-6.3-2.777.473-5.899-.308-6.755-3.369C.42 10.04 0 4.615 0 3.883c0-3.67 3.217-2.517 5.202-1.026" fill="currentColor"/></svg>`;
  const heart = `<svg class="bsky-heart-icon" viewBox="0 0 24 24"><path d="M14 20.408c-.492.308-.903.546-1.192.709-.153.086-.308.17-.463.252h-.002a.75.75 0 0 1-.686 0 16.709 16.709 0 0 1-.465-.252 31.147 31.147 0 0 1-4.803-3.34C3.8 15.572 1 12.331 1 8.513 1 5.052 3.829 2.5 6.736 2.5 9.03 2.5 10.881 3.726 12 5.605 13.12 3.726 14.97 2.5 17.264 2.5 20.17 2.5 23 5.052 23 8.514c0 3.818-2.801 7.06-5.389 9.262A31.146 31.146 0 0 1 14 20.408Z" fill="currentColor"/></svg>`;

  return [
    `<div class="bsky-embed">`,
    `  <div class="bsky-content">`,
    `    <div class="bsky-header">`,
    `      <a href="${escapeHtml(profileUrl)}" target="_blank" rel="noopener noreferrer"><img class="bsky-avatar" src="${escapeHtml(avatar)}" alt="" loading="lazy" /></a>`,
    `      <div class="bsky-author-info">`,
    `        <a class="bsky-name" href="${escapeHtml(profileUrl)}" target="_blank" rel="noopener noreferrer">${escapeHtml(displayName)}</a>`,
    `        <span class="bsky-handle">@${escapeHtml(handle)}</span>`,
    `      </div>`,
    `      <a class="bsky-logo" href="${escapeHtml(postUrl)}" target="_blank" rel="noopener noreferrer" aria-label="View on Bluesky">${butterfly}</a>`,
    `    </div>`,
    `    <div class="bsky-text">${escapeHtml(text)}</div>`,
    `    <div class="bsky-footer">`,
    `      <a class="bsky-date" href="${escapeHtml(postUrl)}" target="_blank" rel="noopener noreferrer">${escapeHtml(date)}</a>`,
    likes > 0 ? `    <span class="bsky-likes">${heart}${likes}</span>` : ``,
    `    </div>`,
    `  </div>`,
    `</div>`,
  ].filter(Boolean).join(`\n`);
}

export default function remarkBluesky() {
  return tree => {
    visit(tree, `containerDirective`, node => {
      if (node.name !== `bsky`) return;

      const {handle, displayName, avatar, date, likes, post} = node.attributes;
      const profileUrl = `https://bsky.app/profile/${handle}`;
      const postUrl = `https://bsky.app/profile/${handle}/post/${post}`;

      const text = node.children
        .filter(c => c.type === `paragraph`)
        .map(p => p.children.map(c => c.value ?? ``).join(``))
        .join(`\n`);

      node.type = `html`;
      node.value = renderEmbed({
        handle,
        displayName,
        avatar,
        text,
        date,
        likes: parseInt(likes, 10) || 0,
        postUrl,
        profileUrl,
      });
      node.children = [];
    });
  };
}
