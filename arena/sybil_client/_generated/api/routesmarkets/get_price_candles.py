from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.price_candles_response import PriceCandlesResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    id: int,
    *,
    resolution: str,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["resolution"] = resolution

    params["from_ms"] = from_ms

    params["to_ms"] = to_ms

    params["before_ms"] = before_ms

    params["limit"] = limit


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/markets/{id}/prices/candles".format(id=quote(str(id), safe=""),),
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> PriceCandlesResponse | None:
    if response.status_code == 200:
        response_200 = PriceCandlesResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[PriceCandlesResponse]:
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
    resolution: str,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[PriceCandlesResponse]:
    """ GET /v1/markets/{id}/prices/candles

    Args:
        id (int):
        resolution (str):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PriceCandlesResponse]
     """


    kwargs = _get_kwargs(
        id=id,
resolution=resolution,
from_ms=from_ms,
to_ms=to_ms,
before_ms=before_ms,
limit=limit,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    resolution: str,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> PriceCandlesResponse | None:
    """ GET /v1/markets/{id}/prices/candles

    Args:
        id (int):
        resolution (str):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PriceCandlesResponse
     """


    return sync_detailed(
        id=id,
client=client,
resolution=resolution,
from_ms=from_ms,
to_ms=to_ms,
before_ms=before_ms,
limit=limit,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    resolution: str,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> Response[PriceCandlesResponse]:
    """ GET /v1/markets/{id}/prices/candles

    Args:
        id (int):
        resolution (str):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[PriceCandlesResponse]
     """


    kwargs = _get_kwargs(
        id=id,
resolution=resolution,
from_ms=from_ms,
to_ms=to_ms,
before_ms=before_ms,
limit=limit,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    resolution: str,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,

) -> PriceCandlesResponse | None:
    """ GET /v1/markets/{id}/prices/candles

    Args:
        id (int):
        resolution (str):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        PriceCandlesResponse
     """


    return (await asyncio_detailed(
        id=id,
client=client,
resolution=resolution,
from_ms=from_ms,
to_ms=to_ms,
before_ms=before_ms,
limit=limit,

    )).parsed
