### Text written by others, as a view shows it (threat model §3.7): who wrote it, and what stands in for
### what is not shown. `$host` is an address in ASCII.

content-origin = { $origin ->
        [agent] Written by an agent
        [owner] Written by you
       *[unknown] Author unknown
    }
content-image = Image left out ({ $host })
content-image-nohost = Image left out
content-link-refused = { $reason ->
        [scheme] link not opened: not https
        [credentials] link not opened: it carries a password
        [too_long] link not opened: too long
       *[invalid] link not opened
    }
content-truncated = The text goes on past { $limit }: this is its beginning.
