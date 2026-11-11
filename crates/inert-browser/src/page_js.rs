//! Scripts the browser runs in web pages. They are Inert's own code, fixed
//! at build time; nothing here comes from a site.

/// Runs at document start in every WebKit page: no rubber-band gap when a
/// page is scrolled past its top or bottom.
pub const BASE: &str = r#"(function(){var add=function(css){var f=function(){try{var s=document.createElement('style');s.textContent=css;(document.head||document.documentElement).appendChild(s);}catch(e){}};if(document.documentElement)f();else document.addEventListener('DOMContentLoaded',f);};window.__inertCss=add;add('html,body{overscroll-behavior:none!important}');})();"#;

/// "Hide my location": geolocation always answers "denied".
pub const HIDE_LOCATION: &str = r#"(function(){try{var deny=function(ok,err){if(err){setTimeout(function(){err({code:1,message:'User denied Geolocation',PERMISSION_DENIED:1,POSITION_UNAVAILABLE:2,TIMEOUT:3});},0);}};var G=window.Geolocation&&Geolocation.prototype;if(G){Object.defineProperty(G,'getCurrentPosition',{value:deny,configurable:false});Object.defineProperty(G,'watchPosition',{value:function(a,b){deny(a,b);return 0;},configurable:false});}}catch(e){}})();"#;

/// Calm mode: hides Shorts, feeds and "more like this" on the sites that
/// are built around them.
pub const CALM: &str = r#"(function(){try{var h=location.hostname,c='';
if(/(^|\.)youtube\.com$/.test(h)){c='ytd-rich-shelf-renderer[is-shorts],ytd-reel-shelf-renderer,ytd-guide-entry-renderer:has(a[title="Shorts"]),ytd-mini-guide-entry-renderer:has(a[title="Shorts"]),a[href^="/shorts"],ytd-browse[page-subtype="home"] ytd-rich-grid-renderer,#related,.ytp-endscreen-content,.ytp-ce-element{display:none!important}';}
else if(/(^|\.)(x|twitter)\.com$/.test(h)){c='[aria-label="Timeline: Trending now"],[data-testid="sidebarColumn"] section,[aria-label="Who to follow"]{display:none!important}';}
else if(/(^|\.)instagram\.com$/.test(h)){c='a[href="/reels/"],a[href="/explore/"],main article~div{display:none!important}';}
else if(/(^|\.)tiktok\.com$/.test(h)){c='[data-e2e="recommend-list-item-container"]{display:none!important}';}
if(c&&window.__inertCss){window.__inertCss(c);}}catch(e){}})();"#;

/// Picture in Picture for the biggest video on the page. Returns "on",
/// "off" or "none".
pub const PIP: &str = r#"(function(){var v=[].slice.call(document.querySelectorAll('video')).filter(function(v){return v.readyState>0;}).sort(function(a,b){return b.clientWidth*b.clientHeight-a.clientWidth*a.clientHeight;})[0];if(!v)return 'none';try{if(v.webkitSetPresentationMode){var on=v.webkitPresentationMode==='picture-in-picture';v.webkitSetPresentationMode(on?'inline':'picture-in-picture');return on?'off':'on';}if(document.pictureInPictureElement){document.exitPictureInPicture();return 'off';}v.requestPictureInPicture();return 'on';}catch(e){return 'none';}})()"#;

/// Turns on the captions a video already carries (and YouTube's own).
/// Returns how many videos got captions, as a string.
pub const CAPTIONS: &str = r#"(function(){var n=0;[].forEach.call(document.querySelectorAll('video'),function(v){var t=[].slice.call(v.textTracks||[]).filter(function(t){return t.kind==='subtitles'||t.kind==='captions';});if(t.length){var on=t.some(function(x){return x.mode==='showing';});t.forEach(function(x,i){x.mode=(!on&&i===0)?'showing':'disabled';});n++;}});var y=document.querySelector('.ytp-subtitles-button');if(y&&y.getAttribute('aria-pressed')!=='true'){y.click();n++;}return String(n);})()"#;

/// The text to read aloud: the selection, else the article, else the page.
pub const READABLE: &str = r#"(function(){var s=String(window.getSelection());if(s.trim().length>1)return s.slice(0,20000);var a=document.querySelector('article')||document.querySelector('main')||document.body;return a?a.innerText.slice(0,20000):'';})()"#;

/// Collects the page's visible text nodes for translation; returns a JSON
/// array of strings and keeps the nodes in `window.__inertT`.
pub const TRANSLATE_COLLECT: &str = r#"(function(){var out=[],nodes=[],total=0;var w=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT,{acceptNode:function(n){var p=n.parentElement;if(!p)return 2;var t=p.tagName;if(t==='SCRIPT'||t==='STYLE'||t==='NOSCRIPT'||t==='CODE'||t==='PRE'||t==='TEXTAREA')return 2;if(!/[A-Za-zÀ-￿]{2}/.test(n.nodeValue))return 2;if(!p.offsetParent&&getComputedStyle(p).position!=='fixed')return 2;return 1;}});var n;while((n=w.nextNode())&&nodes.length<260&&total<14000){nodes.push(n);out.push(n.nodeValue);total+=n.nodeValue.length;}window.__inertT=nodes;return JSON.stringify(out);})()"#;

/// Puts translated strings back into the nodes `TRANSLATE_COLLECT` kept.
pub fn translate_apply(json: &str) -> String {
    format!("(function(t){{var n=window.__inertT||[];for(var i=0;i<n.length&&i<t.length;i++){{if(typeof t[i]==='string')n[i].nodeValue=t[i];}}return String(Math.min(n.length,t.length));}})({json})")
}

/// With the Shield on: YouTube's video ads come from the same servers as
/// the videos, so no network rule can stop them. This skips them as they
/// start (and hides the ad slots around the player), the way uBlock
/// Origin's own YouTube fixes do.
pub const YT_ADS: &str = r#"(function(){if(!/(^|\.)youtube\.com$/.test(location.hostname))return;try{if(window.__inertCss)window.__inertCss('ytd-ad-slot-renderer,ytd-in-feed-ad-layout-renderer,ytd-banner-promo-renderer,ytd-statement-banner-renderer,#player-ads,#masthead-ad,ytd-promoted-sparkles-web-renderer,ytd-companion-slot-renderer,.ytp-ad-overlay-container{display:none!important}');var tick=function(){var p=document.querySelector('.html5-video-player');if(p&&p.classList.contains('ad-showing')){var v=p.querySelector('video');if(v){v.muted=true;if(isFinite(v.duration)&&v.duration>0)v.currentTime=v.duration;}var b=document.querySelector('.ytp-skip-ad-button,.ytp-ad-skip-button,.ytp-ad-skip-button-modern');if(b)b.click();}};setInterval(tick,300);}catch(e){}})();"#;
