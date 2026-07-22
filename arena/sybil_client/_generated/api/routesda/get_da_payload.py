from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from typing import cast



def _get_kwargs(
    height: int,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/da/{height}/payload".format(height=quote(str(height), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | None:
    if response.status_code == 200:
        response_200 = cast(Any, None)
        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse]:
    """ GET /v1/da/{height}/payload

     Canonical witness payload bytes for a retained height, served as application/octet-stream with
    Content-Length. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset
    there are no retained DA artifacts; with pruning disabled rows are retained until the store is
    reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root ->
    witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        height=height,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | None:
    """ GET /v1/da/{height}/payload

     Canonical witness payload bytes for a retained height, served as application/octet-stream with
    Content-Length. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset
    there are no retained DA artifacts; with pruning disabled rows are retained until the store is
    reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root ->
    witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse
     """


    return sync_detailed(
        height=height,
client=client,

    ).parsed

async def asyncio_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse]:
    """ GET /v1/da/{height}/payload

     Canonical witness payload bytes for a retained height, served as application/octet-stream with
    Content-Length. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset
    there are no retained DA artifacts; with pruning disabled rows are retained until the store is
    reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root ->
    witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        height=height,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | None:
    """ GET /v1/da/{height}/payload

     Canonical witness payload bytes for a retained height, served as application/octet-stream with
    Content-Length. Retention follows the store-backed block-history window: with SYBIL_DATA_DIR unset
    there are no retained DA artifacts; with pruning disabled rows are retained until the store is
    reset. Clients MUST verify the SYB-80 section 3 binding chain themselves: payload_root ->
    witness_root -> da_commitment -> L1 RootRecord, and must not trust this server.

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse
     """


    return (await asyncio_detailed(
        height=height,
client=client,

    )).parsed
