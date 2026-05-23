import sys
import json
import base64
import re
import time
import random
from urllib.parse import urlparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from googlenewsdecoder import gnewsdecoder

def decode_offline(source_url):
    try:
        parsed = urlparse(source_url)
        path_parts = parsed.path.strip("/").split("/")
        if not path_parts:
            return None
        base64_str = path_parts[-1]
        padding = len(base64_str) % 4
        if padding:
            base64_str += "=" * (4 - padding)
        decoded_bytes = base64.urlsafe_b64decode(base64_str)
        # Search for HTTP/HTTPS URL
        url_match = re.search(rb'(https?://[^\x00-\x1f\x7f-\xff]+)', decoded_bytes)
        if url_match:
            return url_match.group(1).decode('utf-8', errors='ignore')
    except Exception:
        pass
    return None

def decode_single(url):
    # 1. Try offline decode first
    offline_url = decode_offline(url)
    if offline_url:
        return url, offline_url
        
    # 2. Try online decode with retries and delay
    for attempt in range(3):
        try:
            # Introduce a random delay to prevent rate limits
            time.sleep(random.uniform(0.3, 0.8))
            decoded = gnewsdecoder(url)
            if decoded.get("status"):
                return url, decoded["decoded_url"]
            else:
                # If rate limited or failed, sleep and retry
                time.sleep(1.5 + attempt * 1.5)
        except Exception:
            time.sleep(1.5 + attempt * 1.5)
            
    return url, None

def main():
    if len(sys.argv) < 2:
        sys.exit(1)
        
    urls = sys.argv[1:]
    results = {}
    
    # Use a small number of workers to avoid rate limiting
    with ThreadPoolExecutor(max_workers=3) as executor:
        futures = {executor.submit(decode_single, url): url for url in urls}
        for future in as_completed(futures):
            url, decoded_url = future.result()
            if decoded_url:
                results[url] = decoded_url
                
    print(json.dumps(results))
    sys.exit(0)

if __name__ == "__main__":
    main()
