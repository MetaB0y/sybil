"""
News source lists for different trader personas in the Iran strike market.

Each trader gets a curated list of source domains that reflect a specific
geopolitical lens. Used to filter the GDELT dataset before feeding articles
to NewsTrader instances.
"""

# ── American Trader ──────────────────────────────────────────────────────
# Perspective: US political/news outlets + UK mainstream.

AMERICAN_TRADER_SOURCES = [
    # Tier 1: Major US national
    "yahoo.com", "nypost.com", "foxnews.com", "washingtonexaminer.com",
    "newsweek.com", "cnbc.com", "cbsnews.com", "abcnews.go.com",
    "bostonglobe.com", "forbes.com", "pbs.org", "cnn.com", "edition.cnn.com",
    "us.cnn.com", "nbcnews.com", "time.com", "latimes.com", "upi.com",
    "chicagotribune.com", "npr.org", "theatlantic.com", "abcnews.com",
    "politico.eu",
    # Tier 2: US political/opinion
    "breitbart.com", "aol.com", "foreignpolicy.com", "dailycaller.com",
    "csmonitor.com", "seattletimes.com", "baltimoresun.com",
    "dallasnews.com", "denverpost.com",
    # Tier 3: UK outlets with major US readership
    "dailymail.co.uk", "independent.co.uk", "theguardian.com",
    "bbc.co.uk", "bbc.com",
]


# ── Israeli Trader ────────────────────────────────────────────────────────
# Perspective: Israeli security establishment + Hebrew press.
# Hawkish framing, existential threat lens, security-focused.

ISRAELI_TRADER_SOURCES = {
    # Tier 1 — Core Israeli press (English)
    "jpost.com",              # Jerusalem Post — flagship English-language Israeli paper
    "ynetnews.com",           # Ynet English edition
    "israelnationalnews.com", # Arutz Sheva — right-leaning
    "haaretz.com",            # Haaretz English — left-leaning
    "haaretz.co.il",          # Haaretz Hebrew
    "i24news.tv",             # International English TV channel

    # Tier 2 — Hebrew-language Israeli press
    "ynet.co.il",             # Yedioth Ahronoth online
    "news1.co.il",            # Channel 12/News 1
    "kikar.co.il",            # Kikar HaShabbat — ultra-Orthodox
    "makorrishon.co.il",      # Makor Rishon — right-leaning
    "globes.co.il",           # Globes — business/financial
    "en.globes.co.il",        # Globes English
    "maariv.co.il",           # Maariv
    "mako.co.il",             # Mako (Channel 12)
    "srugim.co.il",           # Srugim — religious Zionist
    "inn.co.il",              # Israel National News Hebrew
    "kipa.co.il",             # Kipa — religious
    "news.walla.co.il",       # Walla News
    "finance.walla.co.il",    # Walla Finance
    "e.walla.co.il",          # Walla English
    "m.news1.co.il",          # News1 mobile
    "ch10.co.il",             # Channel 10
    "pc.co.il",               # PC.co.il (tech/general)
    "themarker.com",          # TheMarker (Haaretz financial)
    "newsru.co.il",           # Russian-language Israeli news
    "aurora-israel.co.il",    # Aurora Israel
    "news.israelinfo.co.il",  # Israel Info

    # Tier 3 — Israeli-adjacent (Jewish/pro-Israel/security)
    "israelherald.com",       # Israel Herald
    "algemeiner.com",         # Algemeiner — Jewish/pro-Israel
    "themedialine.org",       # The Media Line — Mideast journalism
    "gatestoneinstitute.org", # Gatestone — hawkish policy institute
    "jewishinsider.com",      # Jewish Insider
    "forward.com",            # Forward — Jewish American
    "clevelandjewishnews.com",# Cleveland Jewish News
    "americanisraelite.com",  # American Israelite
    "israelvalley.com",       # Israel Valley (French-Israeli)
    "ejpress.org",            # European Jewish Press
    "mishpacha.com",          # Mishpacha Magazine
    "jewishpress.com",        # Jewish Press
    "heritagefl.com",         # Heritage Florida Jewish News
    "sdjewishworld.com",      # San Diego Jewish World
    "jewishvoicesnj.org",     # Jewish Voices NJ
    "ejewishphilanthropy.com",# eJewish Philanthropy
    "honestreporting.com",    # HonestReporting — media watchdog
    "camera.org",             # CAMERA — media watchdog
    "ict.org.il",             # ICT — counter-terrorism institute
    "terrorism-info.org.il",  # Terrorism Info (ITIC)
    "endtime.com",            # Endtime Ministries (Christian Zionist)
    "worthynews.com",         # Worthy News (Christian)
    "whyisrael.org",          # Why Israel
    "thetower.org",           # The Tower
    "defense-update.com",     # Defense Update
    "mondoweiss.net",         # Mondoweiss — critical/left (included for breadth)
    "jweekly.com",            # J Weekly — SF Jewish
}


# ── Arab Trader ───────────────────────────────────────────────────────────
# Perspective: Arab world — Egypt, Gulf, Levant, Iraq, Palestine.

ARAB_TRADER_SOURCES = {
    # ── Pan-Arab / satellite networks ──
    "aljazeera.net",          # Al Jazeera Arabic (Qatar)
    "aljazeera.com",          # Al Jazeera English (Qatar)
    "english.aawsat.com",     # Asharq Al-Awsat English (Saudi-owned, pan-Arab)
    "aawsat.com",             # Asharq Al-Awsat Arabic
    "alarabiya.net",          # Al Arabiya (Saudi)
    "skynewsarabia.com",      # Sky News Arabia (UAE)

    # ── Egypt ──
    "dostor.org",             # Al-Dostor
    "shorouknews.com",        # Shorouk News
    "vetogate.com",           # Veto Gate
    "almasryalyoum.com",      # Al-Masry Al-Youm
    "elwatannews.com",        # El Watan News
    "nile.eg",                # Nile TV / state
    "el-balad.com",           # Sada El Balad

    # ── Palestine ──
    "pnn.ps",                 # Palestine News Network
    "shasha.ps",              # Shasha News
    "alquds.com",             # Al-Quds
    "alfajertv.com",          # Al Fajer TV
    "bokra.net",              # Bokra
    "raya.ps",                # Raya FM
    "alhadath.ps",            # Al Hadath
    "arn.ps",                 # Arn News
    "alwatanvoice.com",       # Al Watan Voice

    # ── Lebanon ──
    "almanar.com.lb",         # Al Manar (Hezbollah)
    "naharnet.com",           # Naharnet
    "yalibnan.com",           # Ya Libnan
    "tayyar.org",             # Free Patriotic Movement
    "cedarnews.net",          # Cedar News
    "lebanon24.com",          # Lebanon 24
    "anbaaonline.com",        # Anbaa Online

    # ── Iraq ──
    "iraqsun.com",            # Iraq Sun
    "kitabat.com",            # Kitabat
    "middle-east-online.com", # Middle East Online
    "azzaman.com",            # Azzaman
    "annabaa.org",            # Annabaa
    "almadapaper.net",        # Al Mada
    "mustaqila.com",          # Al Mustaqila

    # ── Gulf States ──
    "albayan.ae",             # Al Bayan (UAE)
    "alriyadh.com",           # Al Riyadh (Saudi)
    "annaharkw.com",          # Al Nahar (Kuwait)
    "okaz.com.sa",            # Okaz (Saudi)
    "alwatan.com.sa",         # Al Watan (Saudi)
    "omanobserver.om",        # Oman Observer

    # ── Jordan ──
    "jo24.net",               # Jo24
    "addustour.com",          # Ad-Dustour
    "assabeel.net",           # Assabeel
    "khaberni.com",           # Khaberni

    # ── Diaspora / London-based Arab ──
    "alquds.co.uk",           # Al-Quds Al-Arabi (London)
    "middleeastmonitor.com",  # MEMO (London)
    "middleeasteye.net",      # Middle East Eye (London)
    "elaph.com",              # Elaph (London)
}


# ── Anti-US Trader ────────────────────────────────────────────────────────
# Perspective: Iran + Russia + China — the "multipolar" bloc.

ANTI_US_TRADER_SOURCES = {
    # ── Chinese state media ──
    "news.cn",                # Xinhua News Agency
    "xinhuanet.com",          # Xinhua online
    "french.xinhuanet.com",   # Xinhua French
    "spanish.xinhuanet.com",  # Xinhua Spanish
    "kr.xinhuanet.com",       # Xinhua Korean
    "arabic.news.cn",         # Xinhua Arabic
    "globaltimes.cn",         # Global Times
    "chinadaily.com.cn",      # China Daily
    "usa.chinadaily.com.cn",  # China Daily US edition
    "europe.chinadaily.com.cn", # China Daily Europe
    "africa.chinadaily.com.cn", # China Daily Africa
    "world.people.com.cn",    # People's Daily
    "en.people.cn",           # People's Daily English
    "french.people.com.cn",   # People's Daily French
    "french.peopledaily.com.cn", # People's Daily French alt
    "arabic.people.com.cn",   # People's Daily Arabic
    "arabic.peopledaily.com.cn", # People's Daily Arabic alt
    "spanish.peopledaily.com.cn", # People's Daily Spanish
    "military.people.com.cn", # People's Daily military
    "china.org.cn",           # China Internet Information Center
    "french.china.org.cn",    # China.org.cn French
    "en.ce.cn",               # China Economic Net
    "81.cn",                  # PLA Daily
    "eng.chinamil.com.cn",    # China Military Online English
    "chinanews.com.cn",       # China News Service
    "bjreview.com",           # Beijing Review
    "bjreview.com.cn",        # Beijing Review alt
    "qstheory.cn",            # Qiushi
    "banyuetan.org",          # Banyuetan
    "news.cyol.com",          # China Youth Daily

    # ── Chinese major portals & aggregators ──
    "baijiahao.baidu.com",    # Baidu Baijiahao
    "163.com",                # NetEase
    "news.ifeng.com",         # iFeng News
    "finance.ifeng.com",      # iFeng Finance
    "mil.ifeng.com",          # iFeng Military
    "news.sina.com.cn",       # Sina News
    "finance.sina.com.cn",    # Sina Finance
    "mil.news.sina.com.cn",   # Sina Military
    "news.china.com",         # China.com News
    "m.tech.china.com",       # China.com Tech
    "military.china.com",     # China.com Military
    "news.qq.com",            # Tencent QQ News
    "mp.weixin.qq.com",       # WeChat articles
    "sohu.com",               # Sohu

    # ── Chinese financial media ──
    "finance.eastmoney.com",  # East Money
    "nbd.com.cn",             # National Business Daily
    "stcn.com",               # Securities Times
    "yicai.com",              # Yicai (CBN)
    "eeo.com.cn",             # Economic Observer

    # ── Chinese regional / other ──
    "world.qianlong.com",     # Qianlong (Beijing)
    "review.qianlong.com",    # Qianlong commentary
    "export.shobserver.com",  # Shanghai Observer
    "cbgc.scol.com.cn",       # Sichuan Online
    "news.fjsen.com",         # Fujian Southeast Net
    "fjsen.com",              # Fujian Southeast Net alt
    "hinews.cn",              # Hainan News
    "news.ycwb.com",          # Yangcheng Evening News
    "yangtse.com",            # Yangtse Evening Post
    "bjnews.com.cn",          # Beijing News
    "news.dahe.cn",           # Dahe (Henan)
    "cbg.cn",                 # Chongqing Broadcasting
    "news.cnnb.com.cn",       # Ningbo News
    "hkcd.com",               # HK Commercial Daily
    "wenweipo.com",           # Wen Wei Po (pro-Beijing HK)
    "sputniknews.cn",         # Sputnik Chinese edition

    # ── Iranian state / establishment media ──
    "presstv.ir",             # Press TV
    "alalam.ir",              # Al Alam
    "mehrnews.com",           # Mehr News Agency
    "khabaronline.ir",        # Khabar Online
    "negaheiraniannews.ir",   # Negahe Iranian News
    "iranoilgas.com",         # Iran Oil & Gas

    # ── Iranian independent / diaspora (non-opposition) ──
    "balatarin.com",          # Balatarin
    "iranherald.com",         # Iran Herald
    "news.gooya.com",         # Gooya News
    "kar-online.com",         # Kar Online
    "iranpressnews.com",      # Iran Press News

    # ── Russian state media ──
    "rt.com",                 # RT English
    "russian.rt.com",         # RT Russian
    "arabic.rt.com",          # RT Arabic
    "actualidad.rt.com",      # RT Spanish
    "sputnikglobe.com",       # Sputnik Global
    "sputnik.af",             # Sputnik Afghanistan
    "sputnik-georgia.ru",     # Sputnik Georgia (.ru)
    "sputnik-georgia.com",    # Sputnik Georgia (.com)
    "fr.sputniknews.africa",  # Sputnik Africa French
    "tass.ru",                # TASS Russian
    "tass.com",               # TASS English
    "ria.ru",                 # RIA Novosti

    # ── Russian mainstream / establishment ──
    "interfax.ru",            # Interfax
    "iz.ru",                  # Izvestia
    "mk.ru",                  # Moskovsky Komsomolets
    "rbc.ru",                 # RBC
    "kommersant.ru",          # Kommersant
    "vedomosti.ru",           # Vedomosti
    "ng.ru",                  # Nezavisimaya Gazeta
    "inosmi.ru",              # InoSMI
    "english.pravda.ru",      # Pravda English
    "gazeta-pravda.ru",       # Pravda Russian
    "aif.ru",                 # Argumenty i Fakty
    "argumenti.ru",           # Argumenty Nedeli
    "zavtra.ru",              # Zavtra
    "vestikavkaza.ru",        # Vestnik Kavkaza
    "bloknot.ru",             # Bloknot
    "odnako.org",             # Odnako
    "news.mail.ru",           # Mail.ru News
    "bfm.ru",                 # BFM
    "vesti.ru",               # Vesti
    "tvzvezda.ru",            # Zvezda
    "fondsk.ru",              # Strategic Culture Foundation
    "islam-today.ru",         # Islam Today Russia
}


# ── Financial Trader ──────────────────────────────────────────────────────
# Perspective: Markets, economics, oil, sanctions impact.

FINANCIAL_TRADER_SOURCES = {
    # ── Major international financial press ──
    "cnbc.com",               # CNBC
    "cnbcafrica.com",         # CNBC Africa
    "cnbctv18.com",           # CNBC TV18 (India)
    "forbes.com",             # Forbes
    "fortune.com",            # Fortune
    "bloomberg.com",          # Bloomberg
    "bnnbloomberg.ca",        # BNN Bloomberg (Canada)
    "bloomberght.com",        # Bloomberg HT (Turkey)
    "finance.yahoo.com",      # Yahoo Finance
    "businessinsider.com",    # Business Insider
    "foxbusiness.com",        # Fox Business

    # ── Financial wire / data / research ──
    "marketscreener.com",     # MarketScreener
    "zonebourse.com",         # Zonebourse (French)
    "rttnews.com",            # RTT News
    "benzinga.com",           # Benzinga
    "markets.financialcontent.com", # Financial Content
    "marketpulse.com",        # MarketPulse
    "investinglive.com",      # Investing Live
    "investing.com",          # Investing.com
    "investopedia.com",       # Investopedia
    "morningstar.com",        # Morningstar

    # ── Indian financial ──
    "economictimes.indiatimes.com", # Economic Times
    "auto.economictimes.indiatimes.com", # ET Auto
    "moneycontrol.com",       # Moneycontrol
    "livemint.com",           # Mint
    "thehindubusinessline.com", # Hindu Business Line
    "businesstoday.in",       # Business Today
    "business-standard.com",  # Business Standard
    "ibtimes.co.in",          # IBTimes India
    "shippingtribune.com",    # Shipping Tribune

    # ── Energy / oil / commodities ──
    "oilprice.com",           # OilPrice.com
    "rigzone.com",            # Rigzone
    "oilandgas360.com",       # Oil & Gas 360
    "hellenicshippingnews.com", # Hellenic Shipping News
    "maritime-executive.com", # Maritime Executive
    "worldoil.com",           # World Oil

    # ── Alternative / contrarian finance ──
    "zerohedge.com",          # ZeroHedge
    "nakedcapitalism.com",    # Naked Capitalism
    "armstrongeconomics.com", # Armstrong Economics
    "theeconomiccollapseblog.com", # Economic Collapse Blog

    # ── FX / forex ──
    "fxstreet.com",           # FXStreet
    "actionforex.com",        # Action Forex
    "dailyforex.com",         # Daily Forex

    # ── European financial ──
    "boursorama.com",         # Boursorama (France)
    "bankier.pl",             # Bankier (Poland)
    "portfolio.hu",           # Portfolio.hu (Hungary)
    "eleconomista.es",        # El Economista (Spain)
    "dunya.com",              # Dünya (Turkey)
    "londonlovesbusiness.com", # London Loves Business
    "proactiveinvestors.co.uk", # Proactive Investors UK
    "proactiveinvestors.com", # Proactive Investors US
    "ilsole24ore.com",        # Il Sole 24 Ore (Italy)

    # ── IBTimes network ──
    "ibtimes.co.uk",          # IBTimes UK
    "ibtimes.com",            # IBTimes US

    # ── Asia-Pacific financial ──
    "businesstimes.com.sg",   # Business Times Singapore
    "nikkei.com",             # Nikkei
    "asia.nikkei.com",        # Nikkei Asia
    "businessday.co.za",      # Business Day South Africa
    "bangkokpost.com",        # Bangkok Post

    # ── Americas ──
    "financialpost.com",      # Financial Post (Canada)
    "fool.com",               # Motley Fool
}


# ── Balanced Trader ───────────────────────────────────────────────────────
# Perspective: Geographically diverse, mainstream global press.

BALANCED_TRADER_SOURCES = {
    # ── United States ──
    "foreignpolicy.com",      # Foreign Policy
    "cnn.com",                # CNN
    "npr.org",                # NPR
    "theatlantic.com",        # The Atlantic

    # ── United Kingdom ──
    "bbc.com",                # BBC
    "bbc.co.uk",              # BBC UK domain
    "theguardian.com",        # The Guardian
    "independent.co.uk",      # The Independent

    # ── France ──
    "lemonde.fr",             # Le Monde
    "france24.com",           # France 24

    # ── Germany ──
    "dw.com",                 # Deutsche Welle

    # ── India ──
    "timesofindia.indiatimes.com", # Times of India
    "hindustantimes.com",     # Hindustan Times
    "thehindu.com",           # The Hindu
    "indianexpress.com",      # Indian Express

    # ── Turkey ──
    "aa.com.tr",              # Anadolu Agency
    "hurriyet.com.tr",        # Hürriyet
    "dailysabah.com",         # Daily Sabah

    # ── Australia ──
    "abc.net.au",             # ABC Australia
    "sbs.com.au",             # SBS
    "9news.com.au",           # Nine Network

    # ── Canada ──
    "theglobeandmail.com",    # Globe & Mail
    "nationalpost.com",       # National Post

    # ── South Korea ──
    "koreatimes.com",         # Korea Times
    "hani.co.kr",             # Hankyoreh

    # ── Japan ──
    "japantimes.co.jp",       # Japan Times
    "mainichi.jp",            # Mainichi Shimbun

    # ── Southeast Asia ──
    "straitstimes.com",       # Straits Times
    "channelnewsasia.com",    # CNA
    "kompas.com",             # Kompas
    "antaranews.com",         # Antara

    # ── Pakistan ──
    "tribune.com.pk",         # Express Tribune
    "geo.tv",                 # Geo TV

    # ── Ukraine ──
    "ukrinform.ua",           # Ukrinform
    "kyivpost.com",           # Kyiv Post

    # ── Africa ──
    "punchng.com",            # Punch (Nigeria)
    "dailymaverick.co.za",    # Daily Maverick (South Africa)

    # ── Latin America ──
    "elpais.com",             # El País

    # ── Spain ──
    "elperiodico.com",        # El Periódico
}
