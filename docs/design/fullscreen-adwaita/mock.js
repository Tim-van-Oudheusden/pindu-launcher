// Builds the mockups' markup. Only the pictures need it; the spec is adwaita.css.
const S = (d, extra = '') => `<svg viewBox="0 0 16 16" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" ${extra}>${d}</svg>`;
const icons = {
  star: S('<path d="M8 2l1.8 3.9 4.2.5-3.1 2.9.8 4.2L8 11.4l-3.7 2.1.8-4.2L2 6.4l4.2-.5z"/>'),
  drive: S('<rect x="2" y="9.5" width="12" height="4" rx="1.2"/><path d="M3.5 9.5l1.5-6h6l1.5 6"/>'),
  folder: S('<path d="M2.5 4.3a.8.8 0 0 1 .8-.8h3l1.4 1.5h5a.8.8 0 0 1 .8.8v6.4a.8.8 0 0 1-.8.8H3.3a.8.8 0 0 1-.8-.8z"/>'),
  grid: S('<rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1"/><rect x="9" y="2.5" width="4.5" height="4.5" rx="1"/><rect x="2.5" y="9" width="4.5" height="4.5" rx="1"/><rect x="9" y="9" width="4.5" height="4.5" rx="1"/>'),
  home: S('<path d="M2.5 7.5L8 2.8l5.5 4.7v5a.8.8 0 0 1-.8.8H10V9.6H6v3.7H3.3a.8.8 0 0 1-.8-.8z"/>'),
  bell: S('<path d="M4 11V7.5a4 4 0 0 1 8 0V11l1.2 1.5H2.800zM6.800 14h2.400"/>'),
  refresh: S('<path d="M13 8a5 5 0 1 1-1.500-3.500M13 2.500v2.800h-2.800"/>'),
  shuffle: S('<path d="M2 4.500h2.500l5 7H13M2 11.500h2.500l1.500-2M9.500 4.500H13M8.200 6.200l1.300-1.700M11.500 3l1.500 1.500-1.500 1.500M11.500 10l1.500 1.500-1.500 1.500"/>'),
  palette: S('<circle cx="8" cy="8" r="5.800"/><circle cx="5.600" cy="6.300" r=".7" fill="#fff"/><circle cx="8" cy="5" r=".7" fill="#fff"/><circle cx="10.400" cy="6.300" r=".7" fill="#fff"/>'),
  gamepad: S('<path d="M5 4.500h6c1.900 0 3 1.500 3.400 3.800l.4 2.400c.2 1.200-1.200 1.900-2 1l-1.700-1.900H4.900l-1.700 1.900c-.8.900-2.200.2-2-1l.4-2.400C2 6 3.100 4.500 5 4.500z"/><path d="M5.200 6.600v2M4.200 7.600h2"/>'),
  help: S('<circle cx="8" cy="8" r="5.800"/><path d="M6.400 6.400a1.700 1.700 0 1 1 2.400 1.600c-.5.300-.8.600-.8 1.200M8 11.400h.01"/>'),
  gear: S('<circle cx="8" cy="8" r="2.200"/><path d="M8 1.800v1.700M8 12.500v1.700M1.800 8h1.700M12.500 8h1.700M3.600 3.600l1.200 1.200M11.200 11.200l1.200 1.200M3.600 12.400l1.200-1.200M11.200 4.800l1.200-1.200"/>'),
  copy: S('<rect x="5.500" y="5.500" width="8" height="8" rx="1.200"/><path d="M10.500 3.300v-.1a.8.800 0 0 0-.8-.8H3.300a.8.800 0 0 0-.8.800v6.400a.8.800 0 0 0 .8.800h.2"/>'),
  network: S('<path d="M1.500 6.200a9.500 9.500 0 0 1 13 0M4 8.800a6 6 0 0 1 8 0M6.400 11.300a2.500 2.500 0 0 1 3.200 0"/>'),
  bluetooth: S('<path d="M4.500 5l7 6L8 14V2l3.500 3-7 6"/>'),
  volume: S('<path d="M2 6.200h2.400L8 3.200v9.600L4.400 9.800H2zM10.800 5.600a3.400 3.400 0 0 1 0 4.800M12.800 3.800a6 6 0 0 1 0 8.400"/>'),
  next: S('<path d="M6 3l5 5-5 5"/>', 'stroke-width="2"'),
  restore: S('<path d="M6 2.500H2.500V6M10 13.500h3.500V10M2.500 2.500l4 4M13.500 13.500l-4-4"/>'),
  power: S('<path d="M8 2v5.500M4.600 4.200a5 5 0 1 0 6.800 0"/>'),
  battery: '<svg viewBox="0 0 20 16" width="20" height="16" fill="none" stroke="#fff" stroke-width="1.600"><rect x="1.500" y="3.500" width="14" height="9" rx="2"/><path d="M17.500 6.500v3" stroke-linecap="round"/><rect x="3.500" y="5.500" width="8" height="5" rx=".6" fill="#fff" stroke="none"/></svg>',
  play: '<svg viewBox="0 0 16 16" width="16" height="16"><path d="M4.500 2.800v10.400l8.500-5.200z" fill="#fff"/></svg>',
};

function home(focusPlay = true) {
  const tiles = [
    [icons.star, 'Favourites', '12 games', '.home-section-tile.labelled.compact'],
    [icons.drive, 'Installed', '48 games'],
    ['St', 'Steam', '142 games'],
    ['Ep', 'Heroic: Epic', '37 games'],
    ['Lu', 'Lutris', '9 games'],
    [icons.folder, 'Co-op Nights', '14 games'],
    [icons.grid, 'Library', '212 games', '.library-tile.labelled.compact'],
  ].map(([glyph, name, count, note]) => `<div class="home-section-tile labelled compact"${note ? ` data-a="${note}"` : ''}><span class="section-glyph">${glyph}</span><span class="section-text"><span class="section-name">${name}</span><span class="section-count">${count}</span></span></div>`).join('');
  const covers = ['hollow', 'lanternkeep', 'driftline', 'moss', 'starling', 'cinder', 'paper', 'tidewrights', 'hollow']
    .map((g, i) => `<div class="jump-card"${i ? '' : ' data-a=".jump-card (116×174)"'}><img src="art/${g}-cover.jpg" alt=""></div>`).join('');
  return `
  <div class="fullscreen-top-bar inside" data-a=".fullscreen-top-bar (56 px, opaque)">
    <span class="page-tab checked below" data-a=".page-tab:checked">Home</span>
    <span class="page-tab">Library</span>
    <span class="clock">20:20</span>
  </div>
  <div class="home-shelves">
    <div class="home-featured inside right" data-a=".home-featured (AdwCarousel, 244 px)">
      <img src="art/lanternkeep-hero.jpg" alt="">
      <div class="featured-scrim"></div>
      <div class="featured-text">
        <span class="home-eyebrow">Jump Back In</span>
        <span class="featured-title">Lanternkeep</span>
        <span class="home-details"><span>129 hours</span><span>Today</span><span>Steam</span></span>
        <div class="featured-actions">
          <span class="pill suggested-action${focusPlay ? ' focus' : ''}" data-a=".featured-play (app.play)">${icons.play}Play</span>
          <span class="pill below" data-a=".featured-details (win.show-details)">Details</span>
        </div>
      </div>
    </div>
    <div class="carousel-dots"><span class="on"></span><span></span><span></span><span></span><span></span><span></span></div>
    <div class="section-row">${tiles}</div>
    <div class="shelf-header"><span class="shelf-title" data-a=".home-shelf > .shelf-title">Favourites</span><span class="shelf-count">12</span></div>
    <div class="shelf-row">${covers}</div>
  </div>`;
}

function hints(list) {
  return `<div class="hint-bar">${list.map(([g, l]) => `<span class="hint"><span class="hint-glyph">${g}</span>${l}</span>`).join('')}</div>`;
}

function row([icon, label, subtitle, extra, next, focus, note]) {
  return `<div class="row${focus ? ' focus' : ''}${note ? ' below' : ''}"${note ? ` data-a="${note}"` : ''}><span class="icon">${icons[icon]}</span><span class="label"><span>${label}</span>${subtitle ? `<span class="subtitle">${subtitle}</span>` : ''}</span>${extra || ''}${next ? `<span class="next">${icons.next}</span>` : ''}</div>`;
}

const status = `<div class="quick-access-status"><span class="clock">20:20</span><span class="battery">${icons.battery}84 %</span></div>`;
if (location.search.includes('annot')) document.documentElement.classList.add('annot-pending');
window.addEventListener('DOMContentLoaded', () => { if (location.search.includes('annot')) document.body.classList.add('annot'); });

// Panel pages name only the panel's parts.
function quiet(html) { return html.replace(/ data-a="[^"]*"/g, ''); }
