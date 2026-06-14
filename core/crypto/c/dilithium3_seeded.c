/*
 * Provides PQCLEAN_DILITHIUM3_CLEAN_crypto_sign_seed_keypair.
 *
 * This is identical to the keypair() function in PQClean's dilithium3/clean/sign.c
 * except it accepts an explicit 32-byte seed instead of calling randombytes().
 * All referenced symbols are compiled into the pqcrypto-dilithium static archive
 * (libdilithium3_clean.a) and resolved at link time.
 *
 * shake256 is provided by pqcrypto-internals (libpqclean_common.a).
 */

#include <stdint.h>
#include <string.h>

/* Dilithium3 parameters (from params.h) */
#define DL3_N           256
#define DL3_K           6
#define DL3_L           5
#define DL3_SEEDBYTES   32
#define DL3_CRHBYTES    64
#define DL3_TRBYTES     64
#define DL3_PKBYTES     1952

/* Internal PQClean types — must match poly.h / polyvec.h exactly. */
typedef struct { int32_t coeffs[DL3_N]; } dl3_poly;
typedef struct { dl3_poly vec[DL3_L]; }   dl3_polyvecl;
typedef struct { dl3_poly vec[DL3_K]; }   dl3_polyveck;

/* shake256 is in libpqclean_common.a (pqcrypto-internals). */
extern void shake256(uint8_t *out, size_t outlen,
                     const uint8_t *in,  size_t inlen);

/* All PQCLEAN_DILITHIUM3_CLEAN_* symbols are in libdilithium3_clean.a. */
extern void PQCLEAN_DILITHIUM3_CLEAN_polyvec_matrix_expand(
    dl3_polyvecl mat[DL3_K], const uint8_t rho[DL3_SEEDBYTES]);

extern void PQCLEAN_DILITHIUM3_CLEAN_polyvecl_uniform_eta(
    dl3_polyvecl *v, const uint8_t seed[DL3_CRHBYTES], uint16_t nonce);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_uniform_eta(
    dl3_polyveck *v, const uint8_t seed[DL3_CRHBYTES], uint16_t nonce);

extern void PQCLEAN_DILITHIUM3_CLEAN_polyvecl_ntt(dl3_polyvecl *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyvec_matrix_pointwise_montgomery(
    dl3_polyveck *t, const dl3_polyvecl mat[DL3_K], const dl3_polyvecl *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_reduce(dl3_polyveck *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_invntt_tomont(dl3_polyveck *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_add(
    dl3_polyveck *w, const dl3_polyveck *u, const dl3_polyveck *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_caddq(dl3_polyveck *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_polyveck_power2round(
    dl3_polyveck *v1, dl3_polyveck *v0, const dl3_polyveck *v);
extern void PQCLEAN_DILITHIUM3_CLEAN_pack_pk(
    uint8_t pk[DL3_PKBYTES],
    const uint8_t rho[DL3_SEEDBYTES],
    const dl3_polyveck *t1);
extern void PQCLEAN_DILITHIUM3_CLEAN_pack_sk(
    uint8_t *sk,
    const uint8_t rho[DL3_SEEDBYTES],
    const uint8_t tr[DL3_TRBYTES],
    const uint8_t key[DL3_SEEDBYTES],
    const dl3_polyveck *t0,
    const dl3_polyvecl *s1,
    const dl3_polyveck *s2);

/*
 * Deterministic keypair from an explicit 32-byte seed.
 * The seed is expanded via SHAKE-256 identically to how the randomised
 * keypair() function would expand the bytes it gets from randombytes().
 */
int PQCLEAN_DILITHIUM3_CLEAN_crypto_sign_seed_keypair(
    uint8_t *pk, uint8_t *sk, const uint8_t *seed)
{
    uint8_t seedbuf[2 * DL3_SEEDBYTES + DL3_CRHBYTES];
    uint8_t tr[DL3_TRBYTES];
    const uint8_t *rho, *rhoprime, *key;
    dl3_polyvecl mat[DL3_K];
    dl3_polyvecl s1, s1hat;
    dl3_polyveck s2, t1, t0;

    /* Use the caller-provided seed instead of randombytes(). */
    memcpy(seedbuf, seed, DL3_SEEDBYTES);
    shake256(seedbuf, 2 * DL3_SEEDBYTES + DL3_CRHBYTES, seedbuf, DL3_SEEDBYTES);
    rho      = seedbuf;
    rhoprime = rho + DL3_SEEDBYTES;
    key      = rhoprime + DL3_CRHBYTES;

    PQCLEAN_DILITHIUM3_CLEAN_polyvec_matrix_expand(mat, rho);
    PQCLEAN_DILITHIUM3_CLEAN_polyvecl_uniform_eta(&s1, rhoprime, 0);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_uniform_eta(&s2, rhoprime, (uint16_t)DL3_L);

    s1hat = s1;
    PQCLEAN_DILITHIUM3_CLEAN_polyvecl_ntt(&s1hat);
    PQCLEAN_DILITHIUM3_CLEAN_polyvec_matrix_pointwise_montgomery(&t1, mat, &s1hat);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_reduce(&t1);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_invntt_tomont(&t1);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_add(&t1, &t1, &s2);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_caddq(&t1);
    PQCLEAN_DILITHIUM3_CLEAN_polyveck_power2round(&t1, &t0, &t1);
    PQCLEAN_DILITHIUM3_CLEAN_pack_pk(pk, rho, &t1);

    shake256(tr, DL3_TRBYTES, pk, DL3_PKBYTES);
    PQCLEAN_DILITHIUM3_CLEAN_pack_sk(sk, rho, tr, key, &t0, &s1, &s2);

    return 0;
}
