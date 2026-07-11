from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.signed_register_key_request import SignedRegisterKeyRequest
from typing import cast



def _get_kwargs(
    id: int,
    *,
    body: SignedRegisterKeyRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/accounts/{id}/keys/register".format(id=quote(str(id), safe=""),),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | None:
    if response.status_code == 200:
        return None

    if response.status_code == 400:
        return None

    if response.status_code == 403:
        return None

    if response.status_code == 404:
        return None

    if response.status_code == 409:
        return None

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SignedRegisterKeyRequest,

) -> Response[Any]:
    """ POST /v1/accounts/{id}/keys/register — register an additional signing key,
    authorized by a signature from an existing account key (SYB-229).

     Canonical bytes cover the full new key record and the account's current
    key/event digests, domain-separated by `genesis_hash`. The raw-P256 path is
    re-verified by the sequencer; the WebAuthn path is verified at the edge and
    again by the shared verifier before the authenticated intent is forwarded.

    Args:
        id (int):
        body (SignedRegisterKeyRequest): Signed request to register a NEW signing key on an
            account (SYB-229).

            Required whenever the account already has at least one registered key. The
            first key is bootstrapped over the service tier (`POST /v1/accounts/{id}/keys`);
            every subsequent key must be authorized by a signature from an existing
            account key. Like orders/cancels, the canonical payload is domain-separated
            by the chain `genesis_hash` (SYB-224).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: SignedRegisterKeyRequest,

) -> Response[Any]:
    """ POST /v1/accounts/{id}/keys/register — register an additional signing key,
    authorized by a signature from an existing account key (SYB-229).

     Canonical bytes cover the full new key record and the account's current
    key/event digests, domain-separated by `genesis_hash`. The raw-P256 path is
    re-verified by the sequencer; the WebAuthn path is verified at the edge and
    again by the shared verifier before the authenticated intent is forwarded.

    Args:
        id (int):
        body (SignedRegisterKeyRequest): Signed request to register a NEW signing key on an
            account (SYB-229).

            Required whenever the account already has at least one registered key. The
            first key is bootstrapped over the service tier (`POST /v1/accounts/{id}/keys`);
            every subsequent key must be authorized by a signature from an existing
            account key. Like orders/cancels, the canonical payload is domain-separated
            by the chain `genesis_hash` (SYB-224).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

