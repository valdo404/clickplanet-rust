wscat --connect wss://clickplanet.lol/ws/listen | while IFS= read -r line; do
    echo "$line" | head -c -1 | protobuf_inspector
done

curl 'https://clickplanet.lol/api/click' \
  -H 'accept: */*' \
  -H 'accept-language: en-GB,en;q=0.9,fr-FR;q=0.8,fr;q=0.7,en-US;q=0.6' \
  -H 'content-type: application/json' \
  -H 'origin: https://clickplanet.lol' \
  -H 'priority: u=1, i' \
  -H 'referer: https://clickplanet.lol/' \
  -H 'sec-ch-ua: "Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "macOS"' \
  -H 'sec-fetch-dest: empty' \
  -H 'sec-fetch-mode: cors' \
  -H 'sec-fetch-site: same-origin' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36' \
  --data-raw '{"data":[8,213,153,6,18,2,102,114]}'

curl 'https://clickplanet.lol/api/ownerships-by-batch' \
  -H 'accept: */*' \
  -H 'accept-language: en-GB,en;q=0.9,fr-FR;q=0.8,fr;q=0.7,en-US;q=0.6' \
  -H 'content-type: application/json' \
  -H 'origin: https://clickplanet.lol' \
  -H 'priority: u=1, i' \
  -H 'referer: https://clickplanet.lol/' \
  -H 'sec-ch-ua: "Google Chrome";v="131", "Chromium";v="131", "Not_A Brand";v="24"' \
  -H 'sec-ch-ua-mobile: ?0' \
  -H 'sec-ch-ua-platform: "macOS"' \
  -H 'sec-fetch-dest: empty' \
  -H 'sec-fetch-mode: cors' \
  -H 'sec-fetch-site: same-origin' \
  -H 'user-agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36' \
  --data-raw '{"data":[8,145,78,16,161,156,1]}'| jq -r '.data' | base64 -d > output.proto

echo '{"data":[8,213,153,6,18,2,102,114]}' | jq -r '.data[]' | while read -r byte; do printf "\\$(printf '%03o' "$byte")"; done > output.proto

protobuf_inspector < output.proto

# Some playings with tor
websocat -v --socks5 "127.0.0.1:9050" -H "Origin: https://clickplanet.lol" -H "Host: clickplanet.lol"   -H "User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/119.0.0.0 Safari/537.36" \
  -H "Accept-Language: en-US,en;q=0.9" \
  -H "Accept-Encoding: gzip, deflate, br" \
  -H "Connection: keep-alive" --socks5-user-pass : \
  wss://clickplanet.lol/ws/listen
