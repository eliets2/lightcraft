import re,sys,html
def text(fn):
    h=open(fn,encoding='utf-8',errors='replace').read()
    # keep only main content-ish
    h=re.sub(r'(?is)<script.*?</script>',' ',h)
    h=re.sub(r'(?is)<style.*?</style>',' ',h)
    h=re.sub(r'(?is)<nav.*?</nav>',' ',h)
    h=re.sub(r'(?is)<header.*?</header>',' ',h)
    h=re.sub(r'(?is)<footer.*?</footer>',' ',h)
    h=re.sub(r'(?is)<!--.*?-->',' ',h)
    h=re.sub(r'(?is)<(br|/p|/div|/li|/h[1-6]|/tr)>','\n',h)
    h=re.sub(r'(?s)<[^>]+>',' ',h)
    h=html.unescape(h)
    h=re.sub(r'[ \t]+',' ',h)
    h=re.sub(r'\n\s*\n+','\n',h)
    return h.strip()
print(text(sys.argv[1]))
