"""Source domain lists for Texas primary market personas."""

# ── Phase 1 source pools ─────────────────────────────────────────────────────
# These define the two article sets for phase 1 headline filtering.
# Sources chosen based on actual volume in our GDELT dataset.

REPUBLICAN_PRESS = [
    # Core conservative TV/digital
    "foxnews.com",
    "washingtonexaminer.com",
    "breitbart.com",
    "nypost.com",
    "townhall.com",
    "dailycaller.com",
    "freerepublic.com",
    "newsbusters.org",
    "redstate.com",
    "newsmax.com",
    "dailywire.com",
    "nationalreview.com",
    "washingtontimes.com",
    "freebeacon.com",
    "thefederalist.com",
    "oann.com",
    # Shared Texas local (both pools)
    "texastribune.org",
    "dallasnews.com",
]

DEMOCRATIC_PRESS = [
    # Liberal/center-left outlets
    "rawstory.com",
    "thedailybeast.com",
    "cnn.com",
    "us.cnn.com",
    "edition.cnn.com",
    "nbcnews.com",
    "theguardian.com",
    "alternet.org",
    "bostonglobe.com",
    "newsweek.com",
    "mediaite.com",
    "msnbc.com",
    "npr.org",
    "vox.com",
    "theatlantic.com",
    "slate.com",
    "nytimes.com",
    "washingtonpost.com",
    # Shared Texas local (both pools)
    "texastribune.org",
    "dallasnews.com",
]

# ── Trader persona sources (used later for simulation) ────────────────────────

CONSERVATIVE_SOURCES = [
    "foxnews.com",
    "nypost.com",
    "breitbart.com",
    "dailywire.com",
    "nationalreview.com",
    "washingtontimes.com",
    "freebeacon.com",
    "townhall.com",
    "thefederalist.com",
    "dailycaller.com",
    "newsmax.com",
    "oann.com",
    "texastribune.org",
    "dallasnews.com",
    "houstonchronicle.com",
    "statesman.com",
    "expressnews.com",
]

LIBERAL_SOURCES = [
    "nytimes.com",
    "washingtonpost.com",
    "cnn.com",
    "msnbc.com",
    "npr.org",
    "politico.com",
    "thehill.com",
    "axios.com",
    "vox.com",
    "theatlantic.com",
    "slate.com",
    "texastribune.org",
    "dallasnews.com",
    "houstonchronicle.com",
]

TEXAS_LOCAL_SOURCES = [
    "texastribune.org",
    "dallasnews.com",
    "houstonchronicle.com",
    "statesman.com",
    "expressnews.com",
    "star-telegram.com",
    "texasmonthly.com",
    "khou.com",
    "wfaa.com",
    "kxan.com",
    "kvue.com",
    "mysanantonio.com",
    "caller.com",
    "elpasotimes.com",
    "lubbockonline.com",
]

POLITICAL_INSIDER_SOURCES = [
    "politico.com",
    "thehill.com",
    "axios.com",
    "realclearpolitics.com",
    "cookpolitical.com",
    "fivethirtyeight.com",
    "rollcall.com",
    "ballotpedia.org",
    "opensecrets.org",
    "reuters.com",
    "apnews.com",
    "bbc.com",
    "bbc.co.uk",
    "nbcnews.com",
    "abcnews.go.com",
    "cbsnews.com",
]

BALANCED_SOURCES = [
    "reuters.com",
    "apnews.com",
    "bbc.com",
    "bbc.co.uk",
    "nytimes.com",
    "washingtonpost.com",
    "wsj.com",
    "politico.com",
    "thehill.com",
    "axios.com",
    "texastribune.org",
    "dallasnews.com",
    "houstonchronicle.com",
    "foxnews.com",
    "cnn.com",
    "nbcnews.com",
    "realclearpolitics.com",
]
