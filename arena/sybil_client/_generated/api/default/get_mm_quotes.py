from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.mm_quote_snapshot_response import MmQuoteSnapshotResponse
from typing import cast



def _get_kwargs(
    *,
    target_height: int,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["target_height"] = target_height


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/actor/mm-quotes",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> MmQuoteSnapshotResponse | None:
    if response.status_code == 200:
        response_200 = MmQuoteSnapshotResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[MmQuoteSnapshotResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    target_height: int,

) -> Response[MmQuoteSnapshotResponse]:
    """ 
    Args:
        target_height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[MmQuoteSnapshotResponse]
     """


    kwargs = _get_kwargs(
        target_height=target_height,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    target_height: int,

) -> MmQuoteSnapshotResponse | None:
    """ 
    Args:
        target_height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        MmQuoteSnapshotResponse
     """


    return sync_detailed(
        client=client,
target_height=target_height,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    target_height: int,

) -> Response[MmQuoteSnapshotResponse]:
    """ 
    Args:
        target_height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[MmQuoteSnapshotResponse]
     """


    kwargs = _get_kwargs(
        target_height=target_height,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    target_height: int,

) -> MmQuoteSnapshotResponse | None:
    """ 
    Args:
        target_height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        MmQuoteSnapshotResponse
     """


    return (await asyncio_detailed(
        client=client,
target_height=target_height,

    )).parsed
