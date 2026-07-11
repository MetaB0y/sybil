from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.create_api_key_request import CreateApiKeyRequest
from ...models.create_api_key_response import CreateApiKeyResponse
from typing import cast



def _get_kwargs(
    id: int,
    *,
    body: CreateApiKeyRequest,

) -> dict[str, Any]:
    headers: dict[str, Any] = {}


    

    

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/accounts/{id}/api-keys".format(id=quote(str(id), safe=""),),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | CreateApiKeyResponse | None:
    if response.status_code == 200:
        response_200 = CreateApiKeyResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 409:
        response_409 = cast(Any, None)
        return response_409

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | CreateApiKeyResponse]:
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
    body: CreateApiKeyRequest,

) -> Response[Any | CreateApiKeyResponse]:
    """ POST /v1/accounts/{id}/api-keys — create a read API key (signed) (SYB-60)

     The bearer token is returned exactly once; only its blake3 hash is stored.

    Args:
        id (int):
        body (CreateApiKeyRequest): Signed request to create a read-scoped bearer API key
            (SYB-60).

            The bearer token is generated server-side, returned exactly once in the
            response, and never recoverable afterwards (only its blake3 hash is stored).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | CreateApiKeyResponse]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: CreateApiKeyRequest,

) -> Any | CreateApiKeyResponse | None:
    """ POST /v1/accounts/{id}/api-keys — create a read API key (signed) (SYB-60)

     The bearer token is returned exactly once; only its blake3 hash is stored.

    Args:
        id (int):
        body (CreateApiKeyRequest): Signed request to create a read-scoped bearer API key
            (SYB-60).

            The bearer token is generated server-side, returned exactly once in the
            response, and never recoverable afterwards (only its blake3 hash is stored).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | CreateApiKeyResponse
     """


    return sync_detailed(
        id=id,
client=client,
body=body,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: CreateApiKeyRequest,

) -> Response[Any | CreateApiKeyResponse]:
    """ POST /v1/accounts/{id}/api-keys — create a read API key (signed) (SYB-60)

     The bearer token is returned exactly once; only its blake3 hash is stored.

    Args:
        id (int):
        body (CreateApiKeyRequest): Signed request to create a read-scoped bearer API key
            (SYB-60).

            The bearer token is generated server-side, returned exactly once in the
            response, and never recoverable afterwards (only its blake3 hash is stored).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | CreateApiKeyResponse]
     """


    kwargs = _get_kwargs(
        id=id,
body=body,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    body: CreateApiKeyRequest,

) -> Any | CreateApiKeyResponse | None:
    """ POST /v1/accounts/{id}/api-keys — create a read API key (signed) (SYB-60)

     The bearer token is returned exactly once; only its blake3 hash is stored.

    Args:
        id (int):
        body (CreateApiKeyRequest): Signed request to create a read-scoped bearer API key
            (SYB-60).

            The bearer token is generated server-side, returned exactly once in the
            response, and never recoverable afterwards (only its blake3 hash is stored).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | CreateApiKeyResponse
     """


    return (await asyncio_detailed(
        id=id,
client=client,
body=body,

    )).parsed
