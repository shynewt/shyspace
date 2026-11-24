/** Recognising a storage provider from the address someone pasted. */
import { expect, test } from "@playwright/test";
import { describeProvider, parseStorageEndpoint } from "../../src/lib/storageEndpoint";

test("Scaleway: an endpoint gives the region, a bucket address the bucket too", () => {
  expect(parseStorageEndpoint("https://s3.fr-par.scw.cloud")).toEqual({
    endpoint: "https://s3.fr-par.scw.cloud",
    bucket: null,
    region: "fr-par",
    provider: "Scaleway",
    place: "Paris",
  });
  expect(parseStorageEndpoint("https://shyspace-shynewt.s3.fr-par.scw.cloud")).toEqual({
    endpoint: "https://s3.fr-par.scw.cloud",
    bucket: "shyspace-shynewt",
    region: "fr-par",
    provider: "Scaleway",
    place: "Paris",
  });
});

test("AWS: regional endpoints and bucket addresses, and the global one is us-east-1", () => {
  expect(parseStorageEndpoint("s3.eu-west-1.amazonaws.com")).toMatchObject({
    endpoint: "https://s3.eu-west-1.amazonaws.com",
    bucket: null,
    region: "eu-west-1",
    provider: "AWS",
  });
  expect(parseStorageEndpoint("https://notes.s3.eu-central-1.amazonaws.com/")).toMatchObject({
    endpoint: "https://s3.eu-central-1.amazonaws.com",
    bucket: "notes",
    region: "eu-central-1",
  });
  expect(parseStorageEndpoint("https://s3.amazonaws.com")).toMatchObject({ region: "us-east-1", bucket: null });
});

test("Cloudflare R2: the account's endpoint, with region auto", () => {
  expect(parseStorageEndpoint("https://0123abcd.r2.cloudflarestorage.com")).toEqual({
    endpoint: "https://0123abcd.r2.cloudflarestorage.com",
    bucket: null,
    region: "auto",
    provider: "Cloudflare R2",
    place: null,
  });
});

test("Backblaze B2, Hetzner, DigitalOcean Spaces and Wasabi", () => {
  expect(parseStorageEndpoint("https://s3.eu-central-003.backblazeb2.com")).toMatchObject({
    region: "eu-central-003",
    provider: "Backblaze B2",
  });
  expect(parseStorageEndpoint("https://team.fsn1.your-objectstorage.com")).toMatchObject({
    endpoint: "https://fsn1.your-objectstorage.com",
    bucket: "team",
    region: "fsn1",
    provider: "Hetzner",
    place: "Falkenstein",
  });
  expect(parseStorageEndpoint("https://ams3.digitaloceanspaces.com")).toMatchObject({
    region: "ams3",
    provider: "DigitalOcean Spaces",
    place: "Amsterdam",
  });
  expect(parseStorageEndpoint("https://s3.eu-central-2.wasabisys.com")).toMatchObject({
    region: "eu-central-2",
    provider: "Wasabi",
  });
});

test("anything else, half-typed addresses included, is left as typed", () => {
  expect(parseStorageEndpoint("http://localhost:9000")).toBeNull();
  expect(parseStorageEndpoint("https://minio.example.com")).toBeNull();
  expect(parseStorageEndpoint("https://s3.fr-")).toBeNull();
  expect(parseStorageEndpoint("")).toBeNull();
});

test("the hint names the provider and, when known, the place", () => {
  expect(describeProvider({ provider: "Scaleway", place: "Paris" })).toBe("Scaleway, Paris");
  expect(describeProvider({ provider: "AWS", place: null })).toBe("AWS");
});
