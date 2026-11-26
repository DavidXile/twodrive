"""Prove the regression rejects bdab13e's first-valid DNS selection.

Only replace the extracted selection function, retaining the exact earlier
while/first-Ok behavior. Restore production source even if the test crashes.
"""
from pathlib import Path
import subprocess

source = Path('src/network/dns.rs')
original = source.read_text()
start = original.index('async fn prefer_records<F>(')
end = original.index('\nasync fn exchange(', start)
old = '''async fn prefer_records<F>(
    mut calls: FuturesUnordered<F>,
) -> anyhow::Result<(Records, u32)>
where
    F: std::future::Future<Output = anyhow::Result<(Records, u32)>>,
{
    while let Some(result) = calls.next().await {
        if let Ok((records, ttl)) = result {
            return Ok((records, ttl));
        }
    }
    anyhow::bail!("physical DNS resolution failed")
}
'''
try:
    source.write_text(original[:start] + old + original[end:])
    result = subprocess.run(['cargo', 'test', '--locked', 'fast_negative_does_not_hide_later_positive_dns_record'], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if result.returncode == 0 or 'a stale negative hid the published record' not in result.stdout:
        print(result.stdout)
        raise SystemExit('Old DNS selection did not produce the expected regression failure.')
    print('Confirmed old first-valid DNS selection fails: a stale negative hid the published record.')
finally:
    source.write_text(original)
