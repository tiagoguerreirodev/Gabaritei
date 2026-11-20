export async function ciProbe() {
  Promise.resolve("CI probe: floating promise must fail lint");
}
