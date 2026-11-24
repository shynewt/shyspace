/**
 * Recognises a storage provider from the address someone pasted (an endpoint,
 * or a bucket's own address) and what it implies: the endpoint to use, the
 * bucket, the region. A guess to fill the form; the connection test decides.
 */

export type ParsedEndpoint = {
  /** The provider's endpoint, without any bucket in it. */
  endpoint: string;
  /** The bucket, when the address was the bucket's own. */
  bucket: string | null;
  region: string;
  provider: string;
  /** Where the region is, for the hint; null when there's nothing more to say than the provider. */
  place: string | null;
};

type Provider = {
  name: string;
  /** Matches a host; the named groups `bucket` (optional) and `region` or `account` say the rest. */
  host: RegExp;
  endpoint: (groups: Record<string, string>) => string;
  region: (groups: Record<string, string>) => string;
  places?: Record<string, string>;
};

const LABEL = "[a-z0-9](?:[a-z0-9-]*[a-z0-9])?";
const BUCKET = `(?:(?<bucket>${LABEL}(?:\\.${LABEL})*)\\.)?`;

const PROVIDERS: Provider[] = [
  {
    name: "Scaleway",
    host: new RegExp(`^${BUCKET}s3\\.(?<region>${LABEL})\\.scw\\.cloud$`),
    endpoint: ({ region }) => `https://s3.${region}.scw.cloud`,
    region: ({ region }) => region,
    places: { "fr-par": "Paris", "nl-ams": "Amsterdam", "pl-waw": "Warsaw", "it-mil": "Milan" },
  },
  {
    name: "AWS",
    host: new RegExp(`^${BUCKET}s3[.-](?<region>${LABEL})\\.amazonaws\\.com$`),
    endpoint: ({ region }) => `https://s3.${region}.amazonaws.com`,
    region: ({ region }) => region,
    places: {
      "us-east-1": "N. Virginia",
      "us-east-2": "Ohio",
      "us-west-1": "N. California",
      "us-west-2": "Oregon",
      "eu-west-1": "Ireland",
      "eu-west-2": "London",
      "eu-west-3": "Paris",
      "eu-central-1": "Frankfurt",
      "eu-north-1": "Stockholm",
    },
  },
  {
    // The global endpoint, which AWS serves from us-east-1.
    name: "AWS",
    host: new RegExp(`^${BUCKET}s3\\.amazonaws\\.com$`),
    endpoint: () => "https://s3.amazonaws.com",
    region: () => "us-east-1",
    places: { "us-east-1": "N. Virginia" },
  },
  {
    name: "Cloudflare R2",
    host: new RegExp(`^${BUCKET}(?<account>[a-f0-9]+)\\.r2\\.cloudflarestorage\\.com$`),
    endpoint: ({ account }) => `https://${account}.r2.cloudflarestorage.com`,
    region: () => "auto",
  },
  {
    name: "Backblaze B2",
    host: new RegExp(`^${BUCKET}s3\\.(?<region>${LABEL})\\.backblazeb2\\.com$`),
    endpoint: ({ region }) => `https://s3.${region}.backblazeb2.com`,
    region: ({ region }) => region,
  },
  {
    name: "Hetzner",
    host: new RegExp(`^${BUCKET}(?<region>${LABEL})\\.your-objectstorage\\.com$`),
    endpoint: ({ region }) => `https://${region}.your-objectstorage.com`,
    region: ({ region }) => region,
    places: { fsn1: "Falkenstein", nbg1: "Nuremberg", hel1: "Helsinki" },
  },
  {
    name: "DigitalOcean Spaces",
    host: new RegExp(`^${BUCKET}(?<region>${LABEL})\\.digitaloceanspaces\\.com$`),
    endpoint: ({ region }) => `https://${region}.digitaloceanspaces.com`,
    region: ({ region }) => region,
    places: {
      nyc3: "New York",
      sfo3: "San Francisco",
      ams3: "Amsterdam",
      fra1: "Frankfurt",
      lon1: "London",
      sgp1: "Singapore",
      syd1: "Sydney",
    },
  },
  {
    name: "Wasabi",
    host: new RegExp(`^${BUCKET}s3\\.(?<region>${LABEL})\\.wasabisys\\.com$`),
    endpoint: ({ region }) => `https://s3.${region}.wasabisys.com`,
    region: ({ region }) => region,
  },
];

/** What a pasted address says, or null when it's no provider this knows (MinIO, a half-typed address). */
export function parseStorageEndpoint(typed: string): ParsedEndpoint | null {
  const host = hostOf(typed);
  if (!host) return null;
  for (const provider of PROVIDERS) {
    const groups = host.match(provider.host)?.groups;
    if (!groups) continue;
    const region = provider.region(groups);
    return {
      endpoint: provider.endpoint(groups),
      bucket: groups.bucket ?? null,
      region,
      provider: provider.name,
      // R2's "auto" is no place; elsewhere the region's code says where, if no name is known.
      place: provider.places?.[region] ?? (region === "auto" ? null : region),
    };
  }
  return null;
}

/** "Scaleway, Paris", or the provider alone. */
export function describeProvider({ provider, place }: Pick<ParsedEndpoint, "provider" | "place">): string {
  return place ? `${provider}, ${place}` : provider;
}

function hostOf(typed: string): string | null {
  const trimmed = typed.trim();
  if (!trimmed) return null;
  try {
    return new URL(/^[a-z]+:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`).hostname.toLowerCase();
  } catch {
    return null;
  }
}
