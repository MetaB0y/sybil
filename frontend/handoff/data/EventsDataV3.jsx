// Refreshed event data — no crypto on main, new category set.
// Categories order: Politics, Elections, Economy, Tech, Finance, Culture, Climate, Mentions, World, Crypto, Sports

const EVENTS_V3 = [
  {
    id: 'us-2028-dem',
    category: 'Elections',
    title: 'US 2028 · Democratic nominee',
    resolves: 'Aug 27, 2028',
    vol: '4.2M', vol24: '184K', traders: 2104,
    type: 'multi',
    outcomes: [
      { id:'dem-newsom',    label:'Gavin Newsom',  yes:28, delta24:+4.2, vol24:'84K', vol:'1.2M', traders:712, liq:'380K' },
      { id:'dem-aoc',       label:'AOC',           yes:18, delta24:+1.1, vol24:'52K', vol:'780K', traders:512, liq:'240K' },
      { id:'dem-shapiro',   label:'Josh Shapiro',  yes:14, delta24:-0.8, vol24:'38K', vol:'620K', traders:412, liq:'180K' },
      { id:'dem-buttigieg', label:'Pete Buttigieg',yes: 9, delta24:-1.4, vol24:'24K', vol:'420K', traders:284, liq: '92K' },
      { id:'dem-other',     label:'Other',         yes:31, delta24:-2.8, vol24:'64K', vol:'1.1M', traders:612, liq:'320K' },
    ],
  },
  {
    id: 'fed-mar',
    category: 'Economy',
    title: 'Fed rate decision · March FOMC',
    resolves: 'Mar 19, 2026',
    vol: '2.4M', vol24: '312K', traders: 1842,
    type: 'multi',
    outcomes: [
      { id:'fed-25d', label:'25bp cut',  yes:54, delta24:+3.2, vol24:'128K', vol:'984K', traders:712, liq:'410K' },
      { id:'fed-hold',label:'Hold',      yes:31, delta24:-2.1, vol24: '92K', vol:'682K', traders:512, liq:'280K' },
      { id:'fed-50d', label:'50bp cut',  yes:11, delta24:-0.8, vol24: '48K', vol:'412K', traders:380, liq: '92K' },
      { id:'fed-25u', label:'25bp hike', yes: 4, delta24:-0.3, vol24: '12K', vol: '98K', traders:148, liq: '21K' },
    ],
  },
  {
    id: 'best-ai',
    category: 'Tech',
    title: 'Best AI model · year-end 2026 (lmsys)',
    resolves: 'Dec 31, 2026',
    vol: '1.8M', vol24: '142K', traders: 1284,
    type: 'multi',
    outcomes: [
      { id:'ai-anthropic', label:'Anthropic', yes:38, delta24:+5.4, vol24:'64K', vol:'742K', traders:512, liq:'280K' },
      { id:'ai-openai',    label:'OpenAI',    yes:31, delta24:-3.1, vol24:'48K', vol:'612K', traders:412, liq:'240K' },
      { id:'ai-google',    label:'Google',    yes:18, delta24:+1.2, vol24:'24K', vol:'312K', traders:284, liq:'120K' },
      { id:'ai-xai',       label:'xAI',       yes: 8, delta24:-0.4, vol24: '8K', vol:'128K', traders:148, liq: '48K' },
      { id:'ai-other',     label:'Other',     yes: 5, delta24:-1.3, vol24: '4K', vol: '78K', traders: 92, liq: '21K' },
    ],
  },
  {
    id: 'pres-approval',
    category: 'Politics',
    title: 'Presidential approval rating above 45% on July 1?',
    resolves: 'Jul 1, 2026',
    vol: '612K', vol24: '74K', traders: 482,
    type: 'binary',
    outcomes: [{ id:'app-y', label:'Yes', yes:42, delta24:-2.3, vol24:'74K', vol:'612K', traders:482, liq:'210K' }],
  },
  {
    id: 'sp500-eoy',
    category: 'Finance',
    title: 'S&P 500 closes above 6,500 at year-end?',
    resolves: 'Dec 31, 2026',
    vol: '1.1M', vol24: '88K', traders: 712,
    type: 'binary',
    outcomes: [{ id:'sp-y', label:'Yes', yes:61, delta24:+1.4, vol24:'88K', vol:'1.1M', traders:712, liq:'380K' }],
  },
  {
    id: 'oscar-bp',
    category: 'Culture',
    title: 'Oscars 2026 · Best Picture',
    resolves: 'Mar 15, 2026',
    vol: '284K', vol24: '32K', traders: 384,
    type: 'multi',
    outcomes: [
      { id:'os-anora',  label:'Anora',                 yes:34, delta24:+2.1, vol24:'14K', vol:'124K', traders:142, liq:'48K' },
      { id:'os-brutalist',label:'The Brutalist',       yes:22, delta24:-1.4, vol24: '8K', vol: '78K', traders: 92, liq:'31K' },
      { id:'os-conclave',label:'Conclave',             yes:18, delta24:+0.6, vol24: '4K', vol: '52K', traders: 78, liq:'21K' },
      { id:'os-emilia', label:'Emilia Pérez',          yes:12, delta24:-2.8, vol24: '3K', vol: '38K', traders: 48, liq:'14K' },
      { id:'os-other',  label:'Other',                 yes:14, delta24:-0.6, vol24: '3K', vol: '38K', traders: 24, liq:'12K' },
    ],
  },
  {
    id: 'climate-2c',
    category: 'Climate',
    title: 'Global avg temp anomaly exceeds +1.6°C in 2026?',
    resolves: 'Jan 15, 2027',
    vol: '142K', vol24: '12K', traders: 184,
    type: 'binary',
    outcomes: [{ id:'cl-y', label:'Yes', yes:48, delta24:+0.8, vol24:'12K', vol:'142K', traders:184, liq:'58K' }],
  },
  {
    id: 'powell-mention',
    category: 'Mentions',
    title: 'Powell says "data-dependent" 5+ times in next FOMC presser?',
    resolves: 'Mar 19, 2026',
    vol: '78K', vol24: '14K', traders: 92,
    type: 'binary',
    outcomes: [{ id:'pm-y', label:'Yes', yes:67, delta24:+3.4, vol24:'14K', vol: '78K', traders: 92, liq:'24K' }],
  },
  {
    id: 'wc-2026',
    category: 'Sports',
    title: 'FIFA World Cup 2026 · winner',
    resolves: 'Jul 19, 2026',
    vol: '8.1M', vol24: '412K', traders: 4218,
    type: 'multi',
    outcomes: [
      { id:'wc-arg', label:'Argentina', yes:24, delta24:+1.8, vol24:'92K', vol:'1.9M', traders:1284, liq:'620K' },
      { id:'wc-fra', label:'France',    yes:21, delta24:+0.4, vol24:'84K', vol:'1.7M', traders:1102, liq:'540K' },
      { id:'wc-bra', label:'Brazil',    yes:18, delta24:-1.2, vol24:'72K', vol:'1.4M', traders: 891, liq:'480K' },
      { id:'wc-eng', label:'England',   yes:12, delta24:+2.1, vol24:'48K', vol:'820K', traders: 612, liq:'280K' },
      { id:'wc-spa', label:'Spain',     yes:10, delta24:-0.6, vol24:'32K', vol:'612K', traders: 412, liq:'180K' },
      { id:'wc-oth', label:'Other',     yes:15, delta24:-2.5, vol24:'84K', vol:'1.6M', traders: 891, liq:'320K' },
    ],
  },
];

// Reuse SERIES generator
function genSeries3(seed, n=48) {
  let s = seed; const arr = [0.5];
  for (let i=1; i<n; i++) { s = (s*9301+49297)%233280; const r = s/233280; arr.push(Math.max(0.05, Math.min(0.95, arr[i-1]+(r-0.5)*0.06))); }
  return arr;
}
const SERIES_V3 = {};
EVENTS_V3.forEach((e,ei) => e.outcomes.forEach((o,oi) => { SERIES_V3[o.id] = genSeries3((ei+1)*1234 + oi*97, 48); }));

const CATEGORIES_V3 = ['All','Politics','Elections','Economy','Tech','Finance','Culture','Climate','Mentions','World','Crypto','Sports'];

function categoryDotV3(cat) {
  const map = {
    Politics:'#3FB6D9', Elections:'#7DA3F0', Economy:'#E8AA4A', Tech:'#B68FD9',
    Finance:'#5BD99A', Culture:'#E8556C', Climate:'#5BC4D9', Mentions:'#D9A96B',
    World:'#9CB8C9', Crypto:'#F2994A', Sports:'#5BD99A',
  };
  return map[cat] || '#888';
}

Object.assign(window, { EVENTS_V3, SERIES_V3, CATEGORIES_V3, categoryDotV3 });
